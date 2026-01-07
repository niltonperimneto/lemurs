use std::env::args;
use std::error::Error;
use std::fmt::Display;
use std::path::PathBuf;

pub fn usage() {
    print!(
        r###"Lemurs {}
{}
A TUI Display/Login Manager

USAGE: lemurs [OPTIONS] [SUBCOMMAND]

OPTIONS:
    -c, --config <FILE>       A file to replace the default configuration
    -v, --variables <FILE>    A file to replace the set variables
    -h, --help                Print help information
        --no-log
        --preview
        --tty <N>             Override the configured TTY number
        --xsessions <DIR>     Override the path to /usr/share/xsessions
        --wlsessions <DIR>    Override the path to /usr/share/wayland-sessions
        --initial-path <PATH> Override the initial value of the PATH variable
    -V, --version             Print version information

SUBCOMMANDS:
    cache
    envs
    gui-test
    help     Print this message or the help of the given subcommand(s)
"###,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS")
    );
}

pub struct Cli {
    pub preview: bool,
    pub no_log: bool,
    pub tty: Option<u8>,
    pub config: Option<PathBuf>,
    pub variables: Option<PathBuf>,
    pub command: Option<Commands>,
    pub xsessions: Option<PathBuf>,
    pub wlsessions: Option<PathBuf>,
    pub initial_path: Option<String>,
}

pub enum Commands {
    Envs,
    Cache,
    Help,
    Version,
    GuiTest,
}

#[derive(Debug)]
pub enum CliError {
    MissingArgument(&'static str),
    InvalidTTY,
    InvalidArgument(String),
}

impl Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::MissingArgument(flag) => {
                write!(f, "Missing an argument for the given flag '{flag}'")
            }
            CliError::InvalidTTY => {
                write!(f, "Given an invalid TTY number (only 1-12 are allowed)")
            }
            CliError::InvalidArgument(arg) => {
                write!(f, "Given an invalid flag or command '{arg}'")
            }
        }
    }
}

impl Error for CliError {}

impl Cli {
    pub fn parse() -> Result<Self, CliError> {
        Self::parse_args(args().skip(1))
    }

    pub fn parse_args<I>(args: I) -> Result<Self, CliError>
    where
        I: Iterator<Item = String>,
    {
        let mut cli = Cli {
            preview: false,
            no_log: false,
            tty: None,
            config: None,
            variables: None,
            command: None,
            xsessions: None,
            wlsessions: None,
            initial_path: None,
        };

        let mut args = args.enumerate();
        while let Some((i, arg)) = args.next() {
            match (i, arg.trim()) {
                (_, "envs") => cli.command = Some(Commands::Envs),
                (_, "cache") => cli.command = Some(Commands::Cache),
                (_, "help") | (_, "--help") | (_, "-h") => cli.command = Some(Commands::Help),
                (_, "--version") | (_, "-V") => cli.command = Some(Commands::Version),
                (_, "gui-test") => cli.command = Some(Commands::GuiTest),

                (_, "--preview") => cli.preview = true,
                (_, "--no-log") => cli.no_log = true,
                (_, "--tty") => {
                    let (_, arg) = args.next().ok_or(CliError::MissingArgument("tty"))?;
                    let arg = arg.parse().map_err(|_| CliError::InvalidTTY)?;

                    if arg == 0 || arg > 12 {
                        return Err(CliError::InvalidTTY);
                    }

                    cli.tty = Some(arg);
                }
                (_, "--config") | (_, "-c") => {
                    let (_, arg) = args.next().ok_or(CliError::MissingArgument("config"))?;
                    let arg = PathBuf::from(arg);
                    cli.config = Some(arg);
                }
                (_, "--xsessions") => {
                    let (_, arg) = args.next().ok_or(CliError::MissingArgument("xsessions"))?;
                    let arg = PathBuf::from(arg);
                    cli.xsessions = Some(arg);
                }
                (_, "--wlsessions") => {
                    let (_, arg) = args.next().ok_or(CliError::MissingArgument("wlsessions"))?;
                    let arg = PathBuf::from(arg);
                    cli.wlsessions = Some(arg);
                }
                (_, "--variables") | (_, "-v") => {
                    let (_, arg) = args.next().ok_or(CliError::MissingArgument("variables"))?;
                    let arg = PathBuf::from(arg);
                    cli.variables = Some(arg);
                }
                (_, "--initial-path") => {
                    let (_, arg) = args
                        .next()
                        .ok_or(CliError::MissingArgument("initial-path"))?;
                    cli.initial_path = Some(arg);
                }
                (_, arg) => return Err(CliError::InvalidArgument(arg.to_string())),
            }
        }

        Ok(cli)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gui_test_subcommand() {
        let args = vec!["gui-test".to_string()];
        let cli = Cli::parse_args(args.into_iter()).unwrap();
        assert!(matches!(cli.command, Some(Commands::GuiTest)));
    }

    #[test]
    fn test_gui_test_after_flags() {
        let args = vec!["--preview".to_string(), "gui-test".to_string()];
        let cli = Cli::parse_args(args.into_iter()).unwrap();
        assert!(cli.preview);
        assert!(matches!(cli.command, Some(Commands::GuiTest)));
    }

    #[test]
    fn test_gui_test_before_flags() {
        let args = vec!["gui-test".to_string(), "--preview".to_string()];
        let cli = Cli::parse_args(args.into_iter()).unwrap();
        assert!(cli.preview);
        assert!(matches!(cli.command, Some(Commands::GuiTest)));
    }
}
