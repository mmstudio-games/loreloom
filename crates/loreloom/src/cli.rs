use std::{ffi::OsString, path::PathBuf};

use crate::error::AppError;

pub struct Cli {
    pub world_path: PathBuf,
    pub save_path: PathBuf,
    pub mod_paths: Vec<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub headless_input: Option<String>,
    pub help: bool,
}

impl Cli {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, AppError> {
        let mut arguments = arguments.into_iter();
        let _program = arguments.next();
        let mut world_path = PathBuf::from(".");
        let mut save_path = None;
        let mut mod_paths = Vec::new();
        let mut config_path = None;
        let mut headless_input = None;
        let mut help = false;
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--world") => {
                    world_path = arguments
                        .next()
                        .map(PathBuf::from)
                        .ok_or(AppError::Arguments("--world requires a game root"))?;
                }
                Some("--save") => {
                    save_path = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or(AppError::Arguments("--save requires a path"))?,
                    );
                }
                Some("--headless") => {
                    let input = arguments
                        .next()
                        .ok_or(AppError::Arguments("--headless requires UTF-8 input"))?
                        .into_string()
                        .map_err(|_| AppError::Arguments("--headless requires UTF-8 input"))?;
                    headless_input = Some(input);
                }
                Some("--mod") => {
                    mod_paths.push(PathBuf::from(
                        arguments
                            .next()
                            .ok_or(AppError::Arguments("--mod requires a package root"))?,
                    ));
                }
                Some("--config") => {
                    if config_path.is_some() {
                        return Err(AppError::Arguments("--config may only be specified once"));
                    }
                    config_path = Some(PathBuf::from(
                        arguments
                            .next()
                            .ok_or(AppError::Arguments("--config requires a path"))?,
                    ));
                }
                Some("--help" | "-h") => help = true,
                _ => return Err(AppError::Arguments("unknown argument")),
            }
        }
        let save_path = save_path.unwrap_or_else(|| world_path.join(".loreloom/save"));
        Ok(Self {
            world_path,
            save_path,
            mod_paths,
            config_path,
            headless_input,
            help,
        })
    }
}

pub const HELP: &str = "Loreloom agentic world\n\nUsage: loreloom [--world PATH] [--save PATH] [--mod PATH]... --config PATH [--headless INPUT]\n\n  --world PATH      Use this game root (default: current directory)\n  --save PATH       Open or create the SurrealKV save\n  --mod PATH        Enable an explicit directory Mod package root (repeatable)\n  --config PATH     Use strict TOML Provider, budget, Rule, and TUI configuration\n  --headless INPUT  Run one complete player turn without a terminal UI\n  -h, --help        Show this help\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_save_headless_and_help_without_external_state() {
        let parsed = Cli::parse([
            OsString::from("loreloom"),
            OsString::from("--world"),
            OsString::from("game-root"),
            OsString::from("--save"),
            OsString::from("save-dir"),
            OsString::from("--mod"),
            OsString::from("weather-mod"),
            OsString::from("--mod"),
            OsString::from("story-mod"),
            OsString::from("--config"),
            OsString::from("loreloom.toml"),
            OsString::from("--headless"),
            OsString::from("listen"),
        ])
        .expect("valid arguments");
        assert_eq!(parsed.world_path, PathBuf::from("game-root"));
        assert_eq!(parsed.save_path, PathBuf::from("save-dir"));
        assert_eq!(
            parsed.mod_paths,
            [PathBuf::from("weather-mod"), PathBuf::from("story-mod")]
        );
        assert_eq!(parsed.config_path, Some(PathBuf::from("loreloom.toml")));
        assert_eq!(parsed.headless_input.as_deref(), Some("listen"));
        assert!(!parsed.help);

        let help =
            Cli::parse([OsString::from("loreloom"), OsString::from("-h")]).expect("help arguments");
        assert!(help.help);
        let defaults = Cli::parse([
            OsString::from("loreloom"),
            OsString::from("--world"),
            OsString::from("another-world"),
        ])
        .expect("default save path");
        assert_eq!(
            defaults.save_path,
            PathBuf::from("another-world/.loreloom/save")
        );
        assert!(Cli::parse([OsString::from("loreloom"), OsString::from("--unknown")]).is_err());
        assert!(Cli::parse([OsString::from("loreloom"), OsString::from("--world")]).is_err());
        assert!(Cli::parse([OsString::from("loreloom"), OsString::from("--mod")]).is_err());
        assert!(Cli::parse([OsString::from("loreloom"), OsString::from("--config")]).is_err());
    }
}
