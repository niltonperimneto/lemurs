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

fn lower_command_permissions_to_user(
    mut command: Command,
    user_info: &AuthUserInfo<'_>,
) -> Command {
    let uid = user_info.uid;
    let gid = user_info.primary_gid;
    let groups = user_info
        .all_gids
        .iter()
        .cloned()
        .map(Gid::from_raw)
        .collect::<Vec<Gid>>();

    unsafe {
        command.pre_exec(move || {
            // NOTE: The order here is very vital, otherwise permission errors occur
            // This is basically a copy of how the nightly standard library does it.
            nix::unistd::setgroups(&groups)
                .and(nix::unistd::setgid(Gid::from_raw(gid)))
                .and(nix::unistd::setuid(Uid::from_raw(uid)))
                .map_err(|err| err.into())
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
        user_info: &AuthUserInfo<'_>,
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
    // NOTE: Maybe we can do something smart with `with_capacity` here.
    let mut envs = Vec::new();

    match fs::read_dir(&config.wayland.wayland_sessions_path) {
        Ok(paths) => {
            for path in paths {
                let Ok(path) = path else {
                    continue;
                };

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
                        ))
                    }
                    Err(err) => warn!("Skipping '{}', because {err}", path.display()),
                }
            }
        }
        Err(err) => {
            warn!("Failed to read from the wayland sessions folder '{err}'",);
        }
    }

    match fs::read_dir(&config.wayland.scripts_path) {
        Ok(paths) => {
            for path in paths {
                if let Ok(path) = path {
                    let file_name = path.file_name().into_string();

                    if let Ok(file_name) = file_name {
                        if let Ok(metadata) = path.metadata() {
                            if std::os::unix::fs::MetadataExt::mode(&metadata) & 0o111 == 0 {
                                warn!(
                                    "'{}' is not executable and therefore not added as an environment",
                                    file_name
                                );

                                continue;
                            }
                        }

                        info!("Added environment '{file_name}' from lemurs wayland scripts");
                        envs.push((
                            file_name.clone(),
                            PostLoginEnvironment::Wayland {
                                script_path: match path.path().to_str() {
                                    Some(p) => p.to_string(),
                                    None => {
                                        warn!(
                                    "Skipped item because it was impossible to convert to string"
                                );
                                        continue;
                                    }
                                },
                                env_name: file_name.clone(),
                            },
                        ));
                    } else {
                        warn!("Unable to convert OSString to String");
                    }
                } else {
                    warn!("Ignored errorinous path: '{}'", path.unwrap_err());
                }
            }
        }
        Err(_) => {
            warn!(
                "Failed to read from the wayland folder '{}'",
                config.wayland.scripts_path
            );
        }
    }

    if envs.is_empty() || config.environment_switcher.include_tty_shell {
        if envs.is_empty() {
            info!("Added TTY SHELL because no other environments were found");
        }

        envs.push(("TTYSHELL".to_string(), PostLoginEnvironment::Shell));
    }

    envs
}
#[cfg(test)]
mod tests;
