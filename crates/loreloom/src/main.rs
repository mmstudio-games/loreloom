mod cli;
mod client;
mod config;
mod error;
mod world;

use std::process::ExitCode;

use cli::{Cli, HELP};
use client::RuntimeAdapter;
use config::{ProductConfig, ResolvedProductConfig};
use error::AppError;
use world::{WorldSetup, build_world_with};

fn main() -> ExitCode {
    match run_application() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("loreloom: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_application() -> Result<(), AppError> {
    run_application_with(std::env::args_os())
}

fn run_application_with(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(), AppError> {
    let cli = Cli::parse(arguments)?;
    if cli.help {
        print!("{HELP}");
        return Ok(());
    }
    let tokio = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("loreloom-io")
        .build()
        .map_err(AppError::Tokio)?;
    let configured = cli
        .config_path
        .as_deref()
        .map(ProductConfig::load)
        .transpose()?
        .ok_or(AppError::Arguments(
            "--config is required because production play needs a model Provider",
        ))?;
    let ResolvedProductConfig {
        providers,
        tui: tui_config,
    } = tokio.block_on(configured.resolve())?;
    let WorldSetup {
        mut runtime,
        initial_snapshot,
        ..
    } = tokio.block_on(build_world_with(
        &cli.world_path,
        &cli.save_path,
        &cli.mod_paths,
        providers,
    ))?;
    if let Some(input) = cli.headless_input {
        let outcome = tokio.block_on(runtime.handle_player_input(input))?;
        println!("{}", outcome.narration);
        println!("revision {}", outcome.snapshot.revision);
        return Ok(());
    }

    let mut client = RuntimeAdapter::spawn(runtime)?;
    loreloom_tui::run(&mut client, initial_snapshot, tui_config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn missing_provider_secret_fails_before_save_creation() {
        let directory = tempfile::tempdir().expect("application directory");
        let config_path = directory.path().join("loreloom.toml");
        let save_path = directory.path().join("save");
        std::fs::write(
            &config_path,
            r#"
schema_version = 1

[narrator]
api_version = "armillae.llm/v1alpha1"
provider = "openai"
model = "test"

[narrator.credential]
type = "environment"
name = "LORELOOM_TEST_MISSING_NARRATOR_SECRET_5E7615A4"

[npc]
api_version = "armillae.llm/v1alpha1"
provider = "openai"
model = "test"

[npc.credential]
type = "environment"
name = "LORELOOM_TEST_MISSING_NPC_SECRET_A70B67F1"
"#,
        )
        .expect("write config");
        let error = run_application_with([
            OsString::from("loreloom"),
            OsString::from("--config"),
            config_path.into_os_string(),
            OsString::from("--save"),
            save_path.clone().into_os_string(),
            OsString::from("--headless"),
            OsString::from("hello"),
        ])
        .expect_err("missing Secret must fail");
        assert!(matches!(error, AppError::Provider(_)));
        assert!(!save_path.exists());
    }
}
