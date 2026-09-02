use std::{collections::BTreeMap, path::Path};

use loreloom_content::{
    Definition, PlayerBootstrap, PlayerCreationDraft, PlayerCreationFieldType,
    PlayerCreationFieldValue, PlayerCreationMode,
};
use loreloom_core::LongText;
use loreloom_tui::{
    StartupChoiceView, StartupFieldKind, StartupFieldValue, StartupFieldView, StartupFormView,
    StartupModel, StartupPlayerCreationView, StartupPlayerSelection, StartupPresetView,
    StartupSaveView,
};

use crate::{error::AppError, save_catalog::SaveCatalogEntry, world::StartupContent};

pub fn project_startup_model(
    content: &StartupContent,
    saves: &[SaveCatalogEntry],
    config_path: &Path,
    new_game_only: bool,
) -> Result<StartupModel, AppError> {
    let player_creation = match &content.player_creation {
        PlayerCreationMode::Fixed => StartupPlayerCreationView::Fixed,
        PlayerCreationMode::Preset { characters } => {
            let characters = characters
                .iter()
                .map(|character_id| {
                    let Some(Definition::Character(character)) = content
                        .registry
                        .get(character_id)
                        .map(|entry| &entry.definition)
                    else {
                        return Err(AppError::WorldPolicy(
                            "player preset Character is unavailable",
                        ));
                    };
                    let mut details = character
                        .profile
                        .values
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>();
                    details.push(format!(
                        "{} attributes · {} items · {} skills",
                        character.base_attributes.0.len(),
                        character.inventory.len(),
                        character.skills.len()
                    ));
                    Ok(StartupPresetView {
                        character_id: character.id.clone(),
                        display_name: character.display_name.to_string(),
                        summary: character.profile.summary.to_string(),
                        details,
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            StartupPlayerCreationView::Preset { characters }
        }
        PlayerCreationMode::Ugc { form_id } => {
            let Some(Definition::PlayerCreationForm(form)) =
                content.registry.get(form_id).map(|entry| &entry.definition)
            else {
                return Err(AppError::WorldPolicy("player creation Form is unavailable"));
            };
            let fields = form
                .fields
                .iter()
                .map(|field| StartupFieldView {
                    field_id: field.id.clone(),
                    display_name: field.display_name.to_string(),
                    description: field.description.as_ref().map(ToString::to_string),
                    required: field.required,
                    kind: project_field_kind(&field.value_type),
                })
                .collect();
            StartupPlayerCreationView::Ugc {
                form: StartupFormView {
                    form_id: form.id.clone(),
                    display_name: form.display_name.to_string(),
                    description: form.description.to_string(),
                    fields,
                },
            }
        }
    };
    let saves = saves
        .iter()
        .enumerate()
        .map(|(index, entry)| StartupSaveView {
            display_name: entry.display_name.clone(),
            detail: if index == 0 {
                "Most recent".to_owned()
            } else {
                "Compatible save".to_owned()
            },
        })
        .collect();
    Ok(StartupModel {
        world_name: content.world_name.to_string(),
        world_id: content.world_id.to_string(),
        saves,
        packages: content.packages.clone(),
        settings: vec![
            format!("Configuration  {}", config_path.display()),
            "Provider credentials are resolved only after a game is selected.".to_owned(),
            "Edit the TOML file to change providers, budgets, and TUI sizing.".to_owned(),
        ],
        player_creation,
        new_game_only,
    })
}

fn project_field_kind(value: &PlayerCreationFieldType) -> StartupFieldKind {
    match value {
        PlayerCreationFieldType::Text {
            minimum_bytes,
            maximum_bytes,
            default,
        } => StartupFieldKind::Text {
            minimum_bytes: *minimum_bytes,
            maximum_bytes: *maximum_bytes,
            default: default.as_ref().map(ToString::to_string),
        },
        PlayerCreationFieldType::LongText {
            minimum_bytes,
            maximum_bytes,
            default,
        } => StartupFieldKind::LongText {
            minimum_bytes: *minimum_bytes,
            maximum_bytes: *maximum_bytes,
            default: default.as_ref().map(ToString::to_string),
        },
        PlayerCreationFieldType::Integer {
            minimum,
            maximum,
            default,
        } => StartupFieldKind::Integer {
            minimum: *minimum,
            maximum: *maximum,
            default: *default,
        },
        PlayerCreationFieldType::Number {
            minimum,
            maximum,
            default,
        } => StartupFieldKind::Number {
            minimum: *minimum,
            maximum: *maximum,
            default: *default,
        },
        PlayerCreationFieldType::Boolean { default } => {
            StartupFieldKind::Boolean { default: *default }
        }
        PlayerCreationFieldType::SingleChoice { options, default } => {
            StartupFieldKind::SingleChoice {
                options: options.iter().map(project_choice).collect(),
                default: default.clone(),
            }
        }
        PlayerCreationFieldType::MultiChoice {
            minimum_selections,
            maximum_selections,
            options,
            default,
        } => StartupFieldKind::MultiChoice {
            minimum_selections: *minimum_selections,
            maximum_selections: *maximum_selections,
            options: options.iter().map(project_choice).collect(),
            default: default.clone().unwrap_or_default(),
        },
    }
}

fn project_choice(choice: &loreloom_content::PlayerCreationChoice) -> StartupChoiceView {
    StartupChoiceView {
        value: choice.value.clone(),
        display_name: choice.display_name.to_string(),
        description: choice.description.as_ref().map(ToString::to_string),
    }
}

pub fn player_bootstrap(selection: StartupPlayerSelection) -> Result<PlayerBootstrap, AppError> {
    Ok(match selection {
        StartupPlayerSelection::Fixed => PlayerBootstrap::Fixed,
        StartupPlayerSelection::Preset { character_id } => PlayerBootstrap::Preset { character_id },
        StartupPlayerSelection::Ugc(submission) => {
            let values = submission
                .values
                .into_iter()
                .map(|(field_id, value)| {
                    let value = match value {
                        StartupFieldValue::Text(value) => {
                            PlayerCreationFieldValue::Text(LongText::new(value)?)
                        }
                        StartupFieldValue::Integer(value) => {
                            PlayerCreationFieldValue::Integer(value)
                        }
                        StartupFieldValue::Number(value) => PlayerCreationFieldValue::Number(value),
                        StartupFieldValue::Boolean(value) => {
                            PlayerCreationFieldValue::Boolean(value)
                        }
                        StartupFieldValue::SingleChoice(value) => {
                            PlayerCreationFieldValue::SingleChoice(value)
                        }
                        StartupFieldValue::MultiChoice(value) => {
                            PlayerCreationFieldValue::MultiChoice(value)
                        }
                    };
                    Ok((field_id, value))
                })
                .collect::<Result<BTreeMap<_, _>, AppError>>()?;
            PlayerBootstrap::Ugc {
                draft: PlayerCreationDraft {
                    form_id: submission.form_id,
                    values,
                },
            }
        }
    })
}
