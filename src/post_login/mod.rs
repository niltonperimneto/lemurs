use log::{error, info, warn};
use std::error::Error;
use std::fmt::Display;
use std::fs;
use std::path::Path;

use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

use crate::auth::AuthUserInfo;
use crate::config::{Config, ShellLoginFlag};
use crate::env_container::EnvironmentContainer;

use nix::unistd::{Gid, Uid};

use self::wait_with_log::LemursChild;

pub(crate) mod env_variables;
mod wait_with_log;

#[derive(Debug, Clone)]
pub enum PostLoginEnvironment {
    Wayland {
        script_path: String,
        env_name: String,
    },
    Shell,
}

impl PostLoginEnvironment {
    pub fn to_xdg_type(&self) -> &'static str {
        match self {
            Self::Shell => "tty",
            Self::Wayland { .. } => "wayland",
        }
    }

    pub fn to_xdg_desktop(&self) -> Option<&str> {
        match self {
            Self::Wayland { env_name, .. } => Some(env_name),
            Self::Shell => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EnvironmentStartError {
    WaylandStart,
    TTYStart,
}

impl Display for EnvironmentStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WaylandStart => f.write_str("Failed to start Wayland compositor"),
            Self::TTYStart => f.write_str("Failed to start TTY"),
        }
    }
}

impl Error for EnvironmentStartError {}

/// Configures a `Command` to drop privileges to the specified user.
///
/// # Security
///
/// This function executes the following sequence in the child process:
/// 1. `setgroups`: Sets supplementary groups. This MUST be done first because `setuid` might revoke the permission to set groups.
/// 2. `setgid`: Sets the primary GID.
/// 3. `setuid`: Sets the UID (dropping root privileges).
///
/// If any of these steps fail, the child process will abort to prevent running with partial or incorrect privileges (especially root).
fn lower_command_permissions_to_user(mut command: Command, user_info: &AuthUserInfo) -> Command {
    let uid = user_info.uid;
    let gid = user_info.primary_gid;

    // Prepare groups strictly.
    let groups = user_info
        .all_gids
        .iter()
        .cloned()
        .map(Gid::from_raw)
        .collect::<Vec<Gid>>();

    unsafe {
        command.pre_exec(move || {
            // We are now in the child process.
            // Any failure here means we must NOT continue execution.

            // 1. Set supplementary groups
            nix::unistd::setgroups(&groups).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Failed to setgroups: {}", e),
                )
            })?;

            // 2. Set primary GID
            nix::unistd::setgid(Gid::from_raw(gid)).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Failed to setgid: {}", e),
                )
            })?;

            // 3. Set UID (Irreversible drop of privileges if switching from root)
            nix::unistd::setuid(Uid::from_raw(uid)).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Failed to setuid: {}", e),
                )
            })?;

            Ok(())
        });
    }

    command
}

pub enum SpawnedEnvironment {
    Wayland(LemursChild),
    Tty(Child),
}

impl SpawnedEnvironment {
    pub fn pid(&self) -> u32 {
        match self {
            Self::Wayland(client) => client.id(),
            Self::Tty(client) => client.id(),
        }
    }

    pub fn wait(self) {
        info!("Waiting for client to exit");

        match self {
            Self::Wayland(mut client) => match client.wait() {
                Ok(exit_code) => info!("Client exited with exit code `{exit_code}`"),
                Err(err) => error!("Failed to wait for client. Reason: {err}"),
            },
            Self::Tty(mut client) => match client.wait() {
                Ok(exit_code) => info!("Client exited with exit code `{exit_code}`"),
                Err(err) => error!("Failed to wait for client. Reason: {err}"),
            },
        }
    }
}

impl PostLoginEnvironment {
    pub fn spawn(
        &self,
        user_info: &AuthUserInfo,
        _process_env: &mut EnvironmentContainer,
        config: &Config,
    ) -> Result<SpawnedEnvironment, EnvironmentStartError> {
        let shell_login_flag = match config.shell_login_flag {
            ShellLoginFlag::None => None,
            ShellLoginFlag::Short => Some("-l"),
            ShellLoginFlag::Long => Some("--login"),
        };

        let mut client =
            lower_command_permissions_to_user(Command::new(&config.system_shell), user_info);

        let log_path = config.do_log.then_some(Path::new(&config.client_log_path));

        if let Some(shell_login_flag) = shell_login_flag {
            client.arg(shell_login_flag);
        }

        // Apply environment variables
        _process_env.apply_to_command(&mut client);

        client.arg("-c");

        match self {
            PostLoginEnvironment::Wayland { script_path, .. } => {
                info!("Starting Wayland session");

                client.arg(script_path);

                let child = match LemursChild::spawn(client, log_path) {
                    Ok(child) => child,
                    Err(err) => {
                        error!("Failed to start Wayland Compositor. Reason '{err}'");
                        return Err(EnvironmentStartError::WaylandStart);
                    }
                };

                Ok(SpawnedEnvironment::Wayland(child))
            }
            PostLoginEnvironment::Shell => {
                info!("Starting TTY shell");

                let shell = &user_info.shell;
                let child = match client
                    .arg(shell)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .stdin(Stdio::inherit())
                    .spawn()
                {
                    Ok(child) => child,
                    Err(err) => {
                        error!("Failed to start TTY shell. Reason '{err}'");
                        return Err(EnvironmentStartError::TTYStart);
                    }
                };

                Ok(SpawnedEnvironment::Tty(child))
            }
        }
    }
}

fn parse_desktop_entry(path: &Path, _: &Config) -> Result<(String, String), String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            return Err(format!("file cannot be read. Reason: {err}"));
        }
    };

    let desktop_entry = match deentry::DesktopEntry::try_from(&content[..]) {
        Ok(v) => v,
        Err(err) => {
            return Err(format!("file cannot be parsed. Reason: {err}"));
        }
    };

    let Some(desktop_entry) = desktop_entry
        .groups()
        .iter()
        .find(|g| g.name() == "Desktop Entry")
    else {
        return Err("file does not contain 'Desktop Entry' group".to_string());
    };

    let Some(exec) = desktop_entry.get("Exec") else {
        return Err("'Exec' key cannot be found".to_string());
    };

    let exec = match exec.value().as_string() {
        Ok(v) => v,
        Err(err) => {
            return Err(format!(
                "'Exec' key does not contain a string. Reason: {err}"
            ));
        }
    };

    let name = match desktop_entry.get("Name") {
        Some(name) => match name.value().as_string() {
            Ok(v) => v,
            Err(err) => {
                warn!(
                    "Cannot use 'Name' in '{}' because it does not contain a string. Reason: {err}",
                    path.display()
                );

                exec
            }
        },
        None => exec,
    };

    Ok((name.to_string(), exec.to_string()))
}

pub fn get_envs(config: &Config) -> Vec<(String, PostLoginEnvironment)> {
    let mut envs = Vec::new();

    // 0. Add Terminal (TTY Shell) at the top if configured
    if config.environment_switcher.include_tty_shell {
        envs.push(("Terminal".to_string(), PostLoginEnvironment::Shell));
    }

    // 1. Load Wayland Sessions (.desktop files)
    if let Ok(paths) = fs::read_dir(&config.wayland.wayland_sessions_path) {
        for path in paths.flatten() {
            let path = path.path();
            match parse_desktop_entry(&path, config) {
                Ok((name, exec)) => {
                    info!("Added environment '{name}' from wayland sessions");
                    envs.push((
                        name.clone(),
                        PostLoginEnvironment::Wayland {
                            script_path: exec,
                            env_name: name,
                        },
                    ));
                }
                Err(err) => warn!("Skipping '{}': {}", path.display(), err),
            }
        }
    } else {
        warn!(
            "Failed to read wayland sessions directory: '{}'",
            config.wayland.wayland_sessions_path
        );
    }

    // 2. Load Custom Wayland Scripts
    if let Ok(paths) = fs::read_dir(&config.wayland.scripts_path) {
        for entry in paths.flatten() {
            let path = entry.path();

            // Check for execution permission
            let is_executable = match path.metadata() {
                Ok(m) => (std::os::unix::fs::MetadataExt::mode(&m) & 0o111) != 0,
                Err(_) => false,
            };

            if !is_executable {
                warn!("Skipping '{}': Not executable", path.display());
                continue;
            }

            let file_name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n.to_string(),
                None => {
                    warn!("Skipping '{}': Invalid UTF-8 filename", path.display());
                    continue;
                }
            };

            let script_path = match path.to_str() {
                Some(p) => p.to_string(),
                None => {
                    warn!("Skipping '{}': Invalid UTF-8 path", path.display());
                    continue;
                }
            };

            info!("Added environment '{file_name}' from scripts");
            envs.push((
                file_name.clone(),
                PostLoginEnvironment::Wayland {
                    script_path,
                    env_name: file_name,
                },
            ));
        }
    } else {
        warn!(
            "Failed to read scripts directory: '{}'",
            config.wayland.scripts_path
        );
    }

    // 3. Fallback: If no environments found (and TTY wasn't already added), add TTY Shell.
    if envs.is_empty() {
        info!("No environments found. Adding default Terminal (TTY Shell).");
        envs.push(("Terminal".to_string(), PostLoginEnvironment::Shell));
    }

    envs
}
#[cfg(test)]
mod tests;
