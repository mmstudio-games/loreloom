use std::{ffi::OsString, path::PathBuf};

use crate::error::AppError;

pub struct Cli {
    pub save_path: PathBuf,
    pub headless_input: Option<String>,
    pub help: bool,
}

impl Cli {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, AppError> {
        let mut arguments = arguments.into_iter();
        let _program = arguments.next();
        let mut save_path = PathBuf::from(".loreloom/demo-save");
        let mut headless_input = None;
        let mut help = false;
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--save") => {
                    save_path = arguments
                        .next()
                        .map(PathBuf::from)
                        .ok_or(AppError::Arguments("--save requires a path"))?;
                }
                Some("--headless") => {
                    let input = arguments
                        .next()
                        .ok_or(AppError::Arguments("--headless requires UTF-8 input"))?
                        .into_string()
                        .map_err(|_| AppError::Arguments("--headless requires UTF-8 input"))?;
                    headless_input = Some(input);
                }
                Some("--help" | "-h") => help = true,
                _ => return Err(AppError::Arguments("unknown argument")),
            }
        }
        Ok(Self {
            save_path,
            headless_input,
            help,
        })
    }
}

pub const HELP: &str = "Loreloom agentic world demo\n\nUsage: loreloom [--save PATH] [--headless INPUT]\n\n  --save PATH       Open or create the SurrealKV demo save\n  --headless INPUT  Run one complete player turn without a terminal UI\n  -h, --help        Show this help\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_save_headless_and_help_without_external_state() {
        let parsed = Cli::parse([
            OsString::from("loreloom"),
            OsString::from("--save"),
            OsString::from("save-dir"),
            OsString::from("--headless"),
            OsString::from("listen"),
        ])
        .expect("valid arguments");
        assert_eq!(parsed.save_path, PathBuf::from("save-dir"));
        assert_eq!(parsed.headless_input.as_deref(), Some("listen"));
        assert!(!parsed.help);

        let help =
            Cli::parse([OsString::from("loreloom"), OsString::from("-h")]).expect("help arguments");
        assert!(help.help);
        assert!(Cli::parse([OsString::from("loreloom"), OsString::from("--unknown")]).is_err());
    }
}
