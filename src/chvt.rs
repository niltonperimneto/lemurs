//! Adapted From https://github.com/jonay2000/chvt-rs

#[cfg(not(target_env = "musl"))]
type RequestType = libc::c_ulong;
#[cfg(target_env = "musl")]
type RequestType = libc::c_int;

use libc::c_int;
use nix::errno::Errno;
use nix::fcntl::{self, OFlag};
use nix::sys::stat::Mode;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};

const VT_ACTIVATE: RequestType = 0x5606;
const VT_WAITACTIVE: RequestType = 0x5607;

// Request Number to get Keyboard Type
const KDGKBTYPE: RequestType = 0x4B33;

const KB_101: u8 = 0x02;
const KB_84: u8 = 0x01;

#[derive(Debug)]
pub enum ChvtError {
    Activate(Errno),
    WaitActive(Errno),
    OpenConsole(Errno),
    NotAConsole,
    GetFD,
}

impl Error for ChvtError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Activate(e)
            | Self::WaitActive(e)
            | Self::OpenConsole(e) => Some(e),
            _ => None,
        }
    }
}

impl Display for ChvtError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Activate(e) => write!(f, "Failed to activate VT: {e}"),
            Self::WaitActive(e) => write!(f, "Failed to wait for VT to be active: {e}"),
            Self::OpenConsole(e) => write!(f, "Failed to open console: {e}"),
            Self::NotAConsole => write!(f, "File descriptor is not a console"),
            Self::GetFD => write!(f, "Could not find a valid console file descriptor"),
        }
    }
}

/// A wrapper around a file descriptor that may or may not be owned.
/// If it is owned, it will be closed when dropped.
/// If it is shared (borrowed), it will not be closed.
enum ConsoleFd {
    Owned(OwnedFd),
    Shared(RawFd),
}

impl AsRawFd for ConsoleFd {
    fn as_raw_fd(&self) -> RawFd {
        match self {
            Self::Owned(fd) => fd.as_raw_fd(),
            Self::Shared(fd) => *fd,
        }
    }
}

fn is_a_console(fd: RawFd) -> bool {
    let mut arg = 0;
    if unsafe { libc::ioctl(fd, KDGKBTYPE, &mut arg) } > 0 {
        return false;
    }

    (arg == KB_101) || (arg == KB_84)
}

fn open_a_console(filename: &str) -> Result<OwnedFd, ChvtError> {
    for oflag in [OFlag::O_RDWR, OFlag::O_RDONLY, OFlag::O_WRONLY] {
        match fcntl::open(filename, oflag, Mode::empty()) {
            Ok(fd) => {
                // Check if it is a console before wrapping in OwnedFd logic purely
                // But we already have a raw fd.
                if !is_a_console(fd) {
                    let _ = nix::unistd::close(fd);
                    return Err(ChvtError::NotAConsole);
                }

                // Safety: We just opened this FD, so we own it.
                return Ok(unsafe { OwnedFd::from_raw_fd(fd) });
            }
            Err(Errno::EACCES) => continue,
            _ => break,
        }
    }

    // Default error if loop finishes or other error
    Err(ChvtError::OpenConsole(Errno::EIO))
}

fn get_console_fd() -> Result<ConsoleFd, ChvtError> {
    // Try opening new paths first
    let paths = ["/dev/tty", "/dev/tty0", "/dev/vc/0", "/dev/console"];
    
    for path in paths {
        if let Ok(fd) = open_a_console(path) {
            return Ok(ConsoleFd::Owned(fd));
        }
    }

    // Fallback to standard streams if they happen to be consoles
    for fd in 0..3 {
        if is_a_console(fd) {
            return Ok(ConsoleFd::Shared(fd));
        }
    }

    Err(ChvtError::GetFD)
}

pub fn chvt(ttynum: i32) -> Result<(), ChvtError> {
    let console = get_console_fd()?;
    let fd = console.as_raw_fd();

    let activate = unsafe { libc::ioctl(fd, VT_ACTIVATE, ttynum as c_int) };
    if activate < 0 {
        return Err(ChvtError::Activate(Errno::from_raw(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))));
    }

    let wait = unsafe { libc::ioctl(fd, VT_WAITACTIVE, ttynum) };
    if wait < 0 {
        return Err(ChvtError::WaitActive(Errno::from_raw(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))));
    }

    // ConsoleFd is dropped here. 
    // If Owned, it calls close(). If Shared, it does nothing.
    // This fixes the bug where we closed stdin/stdout.
    Ok(())
}
