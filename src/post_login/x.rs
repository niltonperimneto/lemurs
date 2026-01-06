use libc::SIGUSR1;
use rand::Rng;

use std::env;
use std::error::Error;
use std::fmt::Display;
use std::fs::remove_file;
use std::process::{Command, Stdio};
use std::time;
use std::os::unix::io::AsRawFd;

use std::path::{Path, PathBuf};

use log::{error, info};
use nix::sys::signal::{self, SigSet, SigmaskHow, Signal};
use nix::sys::signalfd::SignalFd;

use crate::auth::AuthUserInfo;
use crate::config::Config;
use crate::env_container::EnvironmentContainer;
use crate::post_login::wait_with_log::LemursChild;

#[derive(Debug, Clone)]
pub enum XSetupError {
    DisplayEnvVar,
    HomeEnvVar,
    VTNREnvVar,
    FillingXAuth,
    InvalidUTF8Path,
    XServerStart,
    XServerTimeout,
    XServerPrematureExit,
    SignalMasking(String),
    SignalFdCreation(String),
    PollError(String),
}

impl Display for XSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DisplayEnvVar => f.write_str("`DISPLAY` is not set"),
            Self::HomeEnvVar => f.write_str("`HOME` is not set"),
            Self::VTNREnvVar => f.write_str("`XDG_VTNR` is not set"),
            Self::FillingXAuth => f.write_str("Failed to fill `.Xauthority` file"),
            Self::InvalidUTF8Path => f.write_str("Path that is given is not valid UTF8"),
            Self::XServerStart => f.write_str("Failed to start X server binary"),
            Self::XServerTimeout => f.write_str("Timeout while waiting for X server to start"),
            Self::XServerPrematureExit => {
                f.write_str("X server exited before it signaled to accept connections")
            }
            Self::SignalMasking(e) => write!(f, "Failed to mask signals: {}", e),
            Self::SignalFdCreation(e) => write!(f, "Failed to create SignalFd: {}", e),
            Self::PollError(e) => write!(f, "Failed to poll SignalFd: {}", e),
        }
    }
}

impl Error for XSetupError {}

fn mcookie() -> String {
    let mut rng = rand::rng();
    let cookie: u128 = rng.random();
    format!("{cookie:032x}")
}

pub fn setup_x(
    process_env: &mut EnvironmentContainer,
    user_info: &AuthUserInfo,
    config: &Config,
) -> Result<LemursChild, XSetupError> {
    use std::os::unix::process::CommandExt;

    info!("Start setup of X server");

    let display_value = env::var("DISPLAY").map_err(|_| XSetupError::DisplayEnvVar)?;
    let vtnr_value = env::var("XDG_VTNR").map_err(|_| XSetupError::VTNREnvVar)?;

    // Setup xauth
    let xauth_dir = PathBuf::from(env::var("HOME").map_err(|_| XSetupError::HomeEnvVar)?);
    let xauth_path = xauth_dir.join(".Xauthority");

    info!(
        "Filling `.Xauthority` file at `{xauth_path}`",
        xauth_path = xauth_path.display()
    );

    let _ = remove_file(&xauth_path);

    Command::new(&config.system_shell)
        .arg("-c")
        .arg(format!(
            "{} add {} . {}",
            &config.x11.xauth_path,
            display_value,
            mcookie()
        ))
        .uid(user_info.uid)
        .gid(user_info.primary_gid)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| {
            error!(
                "Failed to fill Xauthority file with `xauth`. Reason: {}",
                err
            );
            XSetupError::FillingXAuth
        })?;

    let xauth_path = xauth_path.to_str().ok_or(XSetupError::InvalidUTF8Path)?;
    process_env.set("XAUTHORITY", xauth_path);

    let doubledigit_vtnr = if vtnr_value.len() == 1 {
        format!("0{vtnr_value}")
    } else {
        vtnr_value
    };

    // Prepare signals for SignalFd
    // We must block SIGUSR1 so it can be handled by SignalFd
    let mut sigset = SigSet::empty();
    sigset.add(Signal::SIGUSR1);
    
    // Block the signal
    signal::pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&sigset), None)
        .map_err(|e| XSetupError::SignalMasking(e.to_string()))?;

    // Create SignalFd
    let signal_fd = SignalFd::new(&sigset)
        .map_err(|e| XSetupError::SignalFdCreation(e.to_string()))?;

    let mut child = Command::new(&config.system_shell);

    let log_path = config
        .do_log
        .then_some(Path::new(&config.x11.xserver_log_path));

    child.arg("-c").arg(format!(
        "{} {display_value} vt{doubledigit_vtnr}",
        &config.x11.xserver_path
    ));

    // Spawn X server
    // Note: The child process will inherit the signal mask, so it will also have SIGUSR1 blocked
    // unless it unblocks it. However, Xorg handles signals by setting handlers, so it should be fine.
    // Ideally, we might want to restore simple mask in pre_exec, but Xorg is generally robust.
    let mut child = match LemursChild::spawn(child, log_path) {
        Ok(c) => c,
        Err(err) => {
            error!("Failed to start X server. Reason: {}", err);
            // Restore signal mask before returning
            let _ = signal::pthread_sigmask(SigmaskHow::SIG_UNBLOCK, Some(&sigset), None);
            return Err(XSetupError::XServerStart);
        }
    };

    // Wait for XServer to signal readiness via SIGUSR1
    let start_time = time::SystemTime::now();
    let sfd = signal_fd.as_raw_fd();
    
    let mut poll_fd = libc::pollfd {
        fd: sfd,
        events: libc::POLLIN,
        revents: 0,
    };

    loop {
        let poll_interval = 100; // ms to check child status
        let remaining_time = if config.x11.xserver_timeout_secs == 0 {
            1000000 // practically infinite
        } else {
            let elapsed = start_time.elapsed().unwrap_or(time::Duration::ZERO).as_millis() as u64;
             let total_ms = (config.x11.xserver_timeout_secs as u64) * 1000;
             if elapsed >= total_ms {
                 0
             } else {
                 total_ms - elapsed
             }
        };

        if remaining_time == 0 && config.x11.xserver_timeout_secs != 0 {
            // Timeout
            // Restore mask
            let _ = signal::pthread_sigmask(SigmaskHow::SIG_UNBLOCK, Some(&sigset), None);
            
             child.kill().unwrap_or_else(|err| {
                error!("Failed to kill Xorg after it timed out. Reason: {err}");
            });
            return Err(XSetupError::XServerTimeout);
        }
        
        // Wait up to poll_interval (or remaining time if less)
        let effective_timeout = (poll_interval as u64).min(remaining_time) as i32;

        let ret = unsafe { libc::poll(&mut poll_fd, 1, effective_timeout) };
        
        if ret < 0 {
             let err = std::io::Error::last_os_error();
             if err.kind() != std::io::ErrorKind::Interrupted {
                 error!("Poll failed: {}", err);
                 // Restore mask
                 let _ = signal::pthread_sigmask(SigmaskHow::SIG_UNBLOCK, Some(&sigset), None);
                 return Err(XSetupError::PollError(err.to_string()));
             }
        } else if ret > 0 {
            if poll_fd.revents & libc::POLLIN != 0 {
                // Signal received!
                // Read it to clear it
                match signal_fd.read_signal() {
                    Ok(Some(info)) => {
                        if info.ssi_signo as i32 == SIGUSR1 {
                            // X Server is ready
                            break;
                        }
                    }
                    Ok(None) => {}, // Should not happen on blocking fd
                    Err(e) => {
                         error!("Failed to read signal: {}", e);
                         // Continue loop or error?
                    }
                }
            }
        }

        // Check if child exited unexpected
        if let Some(status) = child.try_wait().unwrap_or(None) {
            error!(
                "X server died before signaling it was ready to received connections. Status code: {status}."
            );
            // Restore mask
            let _ = signal::pthread_sigmask(SigmaskHow::SIG_UNBLOCK, Some(&sigset), None);
            return Err(XSetupError::XServerPrematureExit);
        }
    }
    
    // X Server is ready.
    // Restore signal mask (Unblock SIGUSR1)
    signal::pthread_sigmask(SigmaskHow::SIG_UNBLOCK, Some(&sigset), None)
        .map_err(|e| XSetupError::SignalMasking(e.to_string()))?;

    if let Ok(x_server_start_time) = start_time.elapsed() {
        info!(
            "It took X server {start_ms}ms to start",
            start_ms = x_server_start_time.as_millis()
        );
    }

    info!("X server is running");

    Ok(child)
}
