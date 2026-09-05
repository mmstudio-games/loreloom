mod cli;
mod client;
mod config;
mod error;
mod mod_selection;
mod save_catalog;
mod startup;
mod world;

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::ExitCode,
};

use cli::{Cli, HELP};
use client::RuntimeAdapter;
use config::{ProductConfig, ResolvedProductConfig};
use error::AppError;
use loreloom_content::PlayerBootstrap;
use loreloom_core::{ModPackageStatus, ModPackageView};
use loreloom_tui::{StartupAction, TuiTerminal};
use mod_selection::{SelectedMod, load as load_mod_selection, save as save_mod_selection};
use save_catalog::{new_save_path, register, scan};
use startup::{player_bootstrap, project_startup_model};
use world::{
    StartupContent, WorldSetup, build_world_with_player, inspect_world_with,
    resolve_installed_mod_paths,
};

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
    let mut tui_terminal = None;
    let mut active_mod_paths = cli.mod_paths.clone();
    if cli.headless_input.is_none() {
        let persisted = load_mod_selection(&cli.world_path)?;
        active_mod_paths.splice(
            0..0,
            resolve_installed_mod_paths(&cli.world_path, &persisted),
        );
    }
    let (save_path, save_display_name, bootstrap) = if cli.headless_input.is_some() {
        let save_path = cli
            .save_path
            .clone()
            .unwrap_or_else(|| cli.world_path.join(".loreloom/save"));
        let display_name = display_name_for_save(&save_path);
        let bootstrap = if save_path.exists() {
            PlayerBootstrap::Fixed
        } else {
            let content = inspect_world_with(&cli.world_path, &active_mod_paths)?;
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
        let mut content = inspect_world_with(&cli.world_path, &active_mod_paths)?;
        let entries = if cli.save_path.is_some() {
            Vec::new()
        } else {
            scan(&cli.world_path, &content.world_id)
        };
        let terminal = tui_terminal.insert(TuiTerminal::open()?);
        let mut open_mods = false;
        let mut notice = None;
        let mut draft_selection = None;
        loop {
            let mut model =
                project_startup_model(&content, &entries, config_path, cli.save_path.is_some())?;
            model.open_mods = open_mods;
            model.notice = notice.take();
            if let Some(selection) = draft_selection.take() {
                project_mod_selection(&mut model.packages.mods, &selection);
            }
            match terminal.run_startup(model, launcher_tui_config)? {
                StartupAction::OpenSave { index } => {
                    let entry = entries
                        .get(index)
                        .ok_or(AppError::SaveCatalog("selected save is unavailable"))?;
                    break (
                        entry.path.clone(),
                        Some(entry.display_name.clone()),
                        PlayerBootstrap::Fixed,
                    );
                }
                StartupAction::NewGame(selection) => {
                    let (path, display_name) = match cli.save_path.clone() {
                        Some(path) => {
                            let display_name = display_name_for_save(&path);
                            (path, Some(display_name))
                        }
                        None => (new_save_path(&cli.world_path)?, None),
                    };
                    break (path, display_name, player_bootstrap(selection)?);
                }
                StartupAction::ApplyMods { enabled } => {
                    let requested = selected_mods(&enabled);
                    let mut candidate_selection = requested.clone();
                    candidate_selection.extend(content.selected_mods_for_paths(&cli.mod_paths));
                    let result =
                        apply_mod_selection(&cli.world_path, &content, &candidate_selection);
                    open_mods = true;
                    match result {
                        Ok((candidate_paths, candidate_content)) => {
                            active_mod_paths = candidate_paths;
                            content = candidate_content;
                            notice = Some("Mod selection saved.".to_owned());
                        }
                        Err(error) => {
                            draft_selection = Some(candidate_selection);
                            notice = Some(format!("Mod selection could not be applied: {error}"));
                        }
                    }
                }
                StartupAction::Quit => return Ok(()),
            }
        }
    };
    let ResolvedProductConfig {
        providers,
        tui: tui_config,
    } = tokio.block_on(configured.resolve())?;
    let WorldSetup {
        mut runtime,
        initial_snapshot,
        appearance,
        save_id,
        world_id,
        ..
    } = tokio.block_on(build_world_with_player(
        &cli.world_path,
        &save_path,
        &active_mod_paths,
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
    if let Some(terminal) = tui_terminal {
        terminal.run_with_appearance(&mut client, initial_snapshot, tui_config, appearance)?;
    } else {
        loreloom_tui::run_with_appearance(&mut client, initial_snapshot, tui_config, appearance)?;
    }
    Ok(())
}

fn selected_mods(packages: &[ModPackageView]) -> BTreeSet<SelectedMod> {
    packages
        .iter()
        .map(|package| SelectedMod {
            mod_id: package.mod_id.clone(),
            version: package.version.clone(),
        })
        .collect()
}

fn project_mod_selection(packages: &mut [ModPackageView], selected: &BTreeSet<SelectedMod>) {
    for package in packages {
        package.status = if selected.contains(&SelectedMod {
            mod_id: package.mod_id.clone(),
            version: package.version.clone(),
        }) {
            ModPackageStatus::Enabled
        } else {
            ModPackageStatus::Installed
        };
    }
}

fn apply_mod_selection(
    world_root: &Path,
    current: &StartupContent,
    selected: &BTreeSet<SelectedMod>,
) -> Result<(Vec<PathBuf>, StartupContent), AppError> {
    let candidate_paths = current.resolve_mod_paths(selected)?;
    let candidate_content = inspect_world_with(world_root, &candidate_paths)?;
    save_mod_selection(world_root, &current.persistent_mods(selected))?;
    Ok((candidate_paths, candidate_content))
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
    fn rejected_mod_selection_keeps_the_previous_persisted_loadout() {
        let directory = tempfile::tempdir().expect("world parent");
        let world = copy_root_world(directory.path(), false);
        let previous = BTreeSet::from([SelectedMod {
            mod_id: "games.loreloom.previous".parse().expect("Mod ID"),
            version: "1.0.0".parse().expect("version"),
        }]);
        save_mod_selection(&world, &previous).expect("previous loadout");
        let content = inspect_world_with(&world, &[]).expect("inspect world");
        let unavailable = BTreeSet::from([SelectedMod {
            mod_id: "games.loreloom.unavailable".parse().expect("Mod ID"),
            version: "1.0.0".parse().expect("version"),
        }]);

        assert!(apply_mod_selection(&world, &content, &unavailable).is_err());
        assert_eq!(
            load_mod_selection(&world).expect("unchanged loadout"),
            previous
        );
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
