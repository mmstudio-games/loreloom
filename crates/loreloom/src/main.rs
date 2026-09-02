mod cli;
mod client;
mod config;
mod error;
mod save_catalog;
mod startup;
mod world;

use std::{path::Path, process::ExitCode};

use cli::{Cli, HELP};
use client::RuntimeAdapter;
use config::{ProductConfig, ResolvedProductConfig};
use error::AppError;
use loreloom_content::PlayerBootstrap;
use loreloom_tui::StartupAction;
use save_catalog::{new_save_path, register, scan};
use startup::{player_bootstrap, project_startup_model};
use world::{WorldSetup, build_world_with_player, inspect_world_with};

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
    let config_path = cli.config_path.as_deref().ok_or(AppError::Arguments(
        "--config is required because production play needs a model Provider",
    ))?;
    let configured = ProductConfig::load(config_path)?;
    let launcher_tui_config = configured.tui_config();
    let (save_path, save_display_name, bootstrap) = if cli.headless_input.is_some() {
        let save_path = cli
            .save_path
            .clone()
            .unwrap_or_else(|| cli.world_path.join(".loreloom/save"));
        let display_name = display_name_for_save(&save_path);
        let bootstrap = if save_path.exists() {
            PlayerBootstrap::Fixed
        } else {
            let content = inspect_world_with(&cli.world_path, &cli.mod_paths)?;
            match content.player_creation {
                loreloom_content::PlayerCreationMode::Fixed => PlayerBootstrap::Fixed,
                loreloom_content::PlayerCreationMode::Preset { .. }
                | loreloom_content::PlayerCreationMode::Ugc { .. } => {
                    return Err(AppError::Arguments(
                        "a headless new game requires fixed player creation; create the save interactively first",
                    ));
                }
            }
        };
        (save_path, Some(display_name), bootstrap)
    } else if let Some(save_path) = cli.save_path.as_ref().filter(|path| path.exists()) {
        (
            save_path.clone(),
            Some(display_name_for_save(save_path)),
            PlayerBootstrap::Fixed,
        )
    } else {
        let content = inspect_world_with(&cli.world_path, &cli.mod_paths)?;
        let entries = if cli.save_path.is_some() {
            Vec::new()
        } else {
            scan(&cli.world_path, &content.world_id)
        };
        let model =
            project_startup_model(&content, &entries, config_path, cli.save_path.is_some())?;
        let action = loreloom_tui::run_startup(model, launcher_tui_config)?;
        match action {
            StartupAction::OpenSave { index } => {
                let entry = entries
                    .get(index)
                    .ok_or(AppError::SaveCatalog("selected save is unavailable"))?;
                (
                    entry.path.clone(),
                    Some(entry.display_name.clone()),
                    PlayerBootstrap::Fixed,
                )
            }
            StartupAction::NewGame(selection) => {
                let (path, display_name) = match cli.save_path.clone() {
                    Some(path) => {
                        let display_name = display_name_for_save(&path);
                        (path, Some(display_name))
                    }
                    None => (new_save_path(&cli.world_path)?, None),
                };
                (path, display_name, player_bootstrap(selection)?)
            }
            StartupAction::Quit => return Ok(()),
        }
    };
    let ResolvedProductConfig {
        providers,
        tui: tui_config,
    } = tokio.block_on(configured.resolve())?;
    let WorldSetup {
        mut runtime,
        initial_snapshot,
        save_id,
        world_id,
        ..
    } = tokio.block_on(build_world_with_player(
        &cli.world_path,
        &save_path,
        &cli.mod_paths,
        providers,
        &bootstrap,
    ))?;
    let save_display_name =
        save_display_name.unwrap_or_else(|| initial_snapshot.player.display_name.to_string());
    if register(&save_path, save_id, world_id, &save_display_name).is_err() {
        eprintln!("loreloom: save catalog metadata could not be updated");
    }
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

fn display_name_for_save(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control))
        .unwrap_or("Loreloom Save")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn copy_root_world(parent: &Path, keep_player_creation: bool) -> std::path::PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let world = parent.join("world");
        std::fs::create_dir_all(world.join("content")).expect("content directory");
        std::fs::create_dir_all(world.join("prompts")).expect("prompt directory");
        std::fs::copy(
            source.join("content/world.json"),
            world.join("content/world.json"),
        )
        .expect("copy content");
        std::fs::copy(
            source.join("prompts/narrator.md"),
            world.join("prompts/narrator.md"),
        )
        .expect("copy narrator prompt");
        std::fs::copy(source.join("prompts/npc.md"), world.join("prompts/npc.md"))
            .expect("copy NPC prompt");
        let mut manifest =
            std::fs::read_to_string(source.join("world.toml")).expect("read world manifest");
        if !keep_player_creation {
            manifest = manifest
                .lines()
                .filter(|line| !line.starts_with("player_creation = "))
                .collect::<Vec<_>>()
                .join("\n");
            manifest.push('\n');
        }
        std::fs::write(world.join("world.toml"), manifest).expect("write world manifest");
        world
    }

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
        let world_path = copy_root_world(directory.path(), false);
        let error = run_application_with([
            OsString::from("loreloom"),
            OsString::from("--world"),
            world_path.into_os_string(),
            OsString::from("--config"),
            config_path.into_os_string(),
            OsString::from("--save"),
            save_path.clone().into_os_string(),
            OsString::from("--headless"),
            OsString::from("hello"),
        ])
        .expect_err("missing Secret must fail");
        let diagnostic = match &error {
            AppError::ProviderSetup(diagnostic) => diagnostic,
            _ => panic!("missing Secret must be a Provider setup failure, got {error}"),
        };
        assert_eq!(diagnostic.slot(), error::ProviderSlot::Narrator);
        assert_eq!(
            diagnostic.issue(),
            error::ProviderSetupIssue::CredentialEnvironmentMissing
        );
        let rendered = format!("{error:?} {error}");
        assert!(rendered.contains("environment LORELOOM_TEST_MISSING_NARRATOR_SECRET_5E7615A4"));
        assert!(rendered.contains("export this variable"));
        assert!(!save_path.exists());
    }

    #[test]
    fn npc_provider_setup_failure_identifies_its_slot_before_save_creation() {
        let directory = tempfile::tempdir().expect("application directory");
        let config_path = directory.path().join("loreloom.toml");
        let save_path = directory.path().join("save");
        std::fs::write(
            &config_path,
            r#"
schema_version = 1

[narrator]
api_version = "armillae.llm/v1alpha1"
provider = "ollama"
model = "test"

[npc]
api_version = "armillae.llm/v1alpha1"
provider = "deepseek"
model = "test"

[npc.credential]
type = "environment"
name = "LORELOOM_TEST_MISSING_NPC_SECRET_D3C511BE"
"#,
        )
        .expect("write config");
        let world_path = copy_root_world(directory.path(), false);
        let error = run_application_with([
            OsString::from("loreloom"),
            OsString::from("--world"),
            world_path.into_os_string(),
            OsString::from("--config"),
            config_path.into_os_string(),
            OsString::from("--save"),
            save_path.clone().into_os_string(),
            OsString::from("--headless"),
            OsString::from("hello"),
        ])
        .expect_err("missing NPC Secret must fail");

        let diagnostic = match &error {
            AppError::ProviderSetup(diagnostic) => diagnostic,
            _ => panic!("missing Secret must be a Provider setup failure, got {error}"),
        };
        assert_eq!(diagnostic.slot(), error::ProviderSlot::Npc);
        assert_eq!(
            diagnostic.issue(),
            error::ProviderSetupIssue::CredentialEnvironmentMissing
        );
        assert!(!save_path.exists());
    }

    #[test]
    fn headless_new_game_rejects_interactive_player_creation_before_provider_setup() {
        let directory = tempfile::tempdir().expect("application directory");
        let world = copy_root_world(directory.path(), true);
        let config_path = directory.path().join("loreloom.toml");
        std::fs::write(
            &config_path,
            r#"
schema_version = 1

[narrator]
api_version = "armillae.llm/v1alpha1"
provider = "ollama"
model = "test"

[npc]
api_version = "armillae.llm/v1alpha1"
provider = "ollama"
model = "test"
"#,
        )
        .expect("write config");
        let save_path = directory.path().join("save");

        let error = run_application_with([
            OsString::from("loreloom"),
            OsString::from("--world"),
            world.into_os_string(),
            OsString::from("--config"),
            config_path.into_os_string(),
            OsString::from("--save"),
            save_path.clone().into_os_string(),
            OsString::from("--headless"),
            OsString::from("hello"),
        ])
        .expect_err("interactive creation must not be guessed in headless mode");

        assert!(matches!(error, AppError::Arguments(_)));
        assert!(!save_path.exists());
    }
}
