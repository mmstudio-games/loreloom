use std::collections::BTreeSet;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use loreloom_core::{ContentDefinitionId, Fixed, ModId, ModPackageStatus, PackageCatalogView};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    InputEditor, TuiConfig, TuiError, TuiTerminal,
    render::{format_fixed, push_mod_package},
};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupModel {
    pub world_name: String,
    pub world_id: String,
    pub saves: Vec<StartupSaveView>,
    pub packages: PackageCatalogView,
    pub settings: Vec<String>,
    pub setting_fields: Vec<StartupSettingView>,
    pub settings_draft: Option<Vec<StartupSettingView>>,
    pub open_settings: bool,
    pub open_saves: bool,
    pub recovery_error: Option<String>,
    pub player_creation: StartupPlayerCreationView,
    pub new_game_only: bool,
    pub open_mods: bool,
    pub notice: Option<String>,
}

/// A non-secret Host setting. Validation and persistence belong to the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupSettingView {
    pub key: String,
    pub value: String,
    pub help: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupSaveView {
    pub display_name: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPlayerCreationView {
    Fixed,
    Preset { characters: Vec<StartupPresetView> },
    Ugc { form: StartupFormView },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupPresetView {
    pub character_id: ContentDefinitionId,
    pub display_name: String,
    pub summary: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupFormView {
    pub form_id: ContentDefinitionId,
    pub display_name: String,
    pub description: String,
    pub fields: Vec<StartupFieldView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupFieldView {
    pub field_id: ContentDefinitionId,
    pub display_name: String,
    pub description: Option<String>,
    pub required: bool,
    pub kind: StartupFieldKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupFieldKind {
    Text {
        minimum_bytes: u32,
        maximum_bytes: u32,
        default: Option<String>,
    },
    LongText {
        minimum_bytes: u32,
        maximum_bytes: u32,
        default: Option<String>,
    },
    Integer {
        minimum: i64,
        maximum: i64,
        default: Option<i64>,
    },
    Number {
        minimum: Fixed,
        maximum: Fixed,
        default: Option<Fixed>,
    },
    Boolean {
        default: bool,
    },
    SingleChoice {
        options: Vec<StartupChoiceView>,
        default: Option<ContentDefinitionId>,
    },
    MultiChoice {
        minimum_selections: u32,
        maximum_selections: u32,
        options: Vec<StartupChoiceView>,
        default: BTreeSet<ContentDefinitionId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupChoiceView {
    pub value: ContentDefinitionId,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupAction {
    OpenSave {
        index: usize,
    },
    DeleteSave {
        index: usize,
    },
    NewGame(StartupPlayerSelection),
    ApplyMods {
        enabled: Vec<loreloom_core::ModPackageView>,
    },
    ApplySettings {
        fields: Vec<StartupSettingView>,
    },
    RetryStartup,
    BackToLauncher,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPlayerSelection {
    Fixed,
    Preset { character_id: ContentDefinitionId },
    Ugc(StartupFormSubmission),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupFormSubmission {
    pub form_id: ContentDefinitionId,
    pub values: Vec<(ContentDefinitionId, StartupFieldValue)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupFieldValue {
    Text(String),
    Integer(i64),
    Number(Fixed),
    Boolean(bool),
    SingleChoice(ContentDefinitionId),
    MultiChoice(BTreeSet<ContentDefinitionId>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupPage {
    Recovery,
    Main,
    Saves,
    Mods,
    Settings,
    Presets,
    Form,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FormValueState {
    Text(String),
    Integer(String),
    Number(String),
    Boolean(bool),
    SingleChoice(Option<ContentDefinitionId>),
    MultiChoice(BTreeSet<ContentDefinitionId>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FormValidationError {
    field_index: usize,
    notice: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupFormState {
    current: usize,
    values: Vec<FormValueState>,
    option_cursors: Vec<usize>,
    validation_errors: Vec<Option<String>>,
    editor: InputEditor,
}

impl StartupFormState {
    fn new(form: &StartupFormView) -> Self {
        let values = form
            .fields
            .iter()
            .map(|field| match &field.kind {
                StartupFieldKind::Text { default, .. }
                | StartupFieldKind::LongText { default, .. } => {
                    FormValueState::Text(default.clone().unwrap_or_default())
                }
                StartupFieldKind::Integer { default, .. } => FormValueState::Integer(
                    default.map_or_else(String::new, |value| value.to_string()),
                ),
                StartupFieldKind::Number { default, .. } => {
                    FormValueState::Number(default.map_or_else(String::new, format_fixed))
                }
                StartupFieldKind::Boolean { default } => FormValueState::Boolean(*default),
                StartupFieldKind::SingleChoice { default, .. } => {
                    FormValueState::SingleChoice(default.clone())
                }
                StartupFieldKind::MultiChoice { default, .. } => {
                    FormValueState::MultiChoice(default.clone())
                }
            })
            .collect::<Vec<_>>();
        let editor = values
            .first()
            .and_then(editor_text)
            .and_then(|value| InputEditor::with_text(value).ok())
            .unwrap_or_default();
        Self {
            current: 0,
            option_cursors: vec![0; values.len()],
            validation_errors: vec![None; values.len()],
            values,
            editor,
        }
    }

    fn store_editor(&mut self) {
        if let Some(value) = self.values.get_mut(self.current) {
            match value {
                FormValueState::Text(text)
                | FormValueState::Integer(text)
                | FormValueState::Number(text) => text.clone_from(&self.editor.text().to_owned()),
                FormValueState::Boolean(_)
                | FormValueState::SingleChoice(_)
                | FormValueState::MultiChoice(_) => {}
            }
        }
    }

    fn load_editor(&mut self) {
        self.editor = self
            .values
            .get(self.current)
            .and_then(editor_text)
            .and_then(|value| InputEditor::with_text(value).ok())
            .unwrap_or_default();
    }

    fn select(&mut self, index: usize) {
        self.store_editor();
        self.current = index.min(self.values.len().saturating_sub(1));
        self.load_editor();
    }

    fn current_validation_notice(&self) -> Option<String> {
        self.validation_errors
            .get(self.current)
            .and_then(Clone::clone)
    }
}

fn editor_text(value: &FormValueState) -> Option<&str> {
    match value {
        FormValueState::Text(text)
        | FormValueState::Integer(text)
        | FormValueState::Number(text) => Some(text),
        FormValueState::Boolean(_)
        | FormValueState::SingleChoice(_)
        | FormValueState::MultiChoice(_) => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupApp {
    delete_confirmation: Option<(usize, bool)>,
    settings_editor: Option<InputEditor>,
    initial_settings: Vec<StartupSettingView>,
    pub model: StartupModel,
    pub page: StartupPage,
    pub selected: usize,
    pub notice: Option<String>,
    form: Option<StartupFormState>,
    initial_enabled_mods: BTreeSet<(ModId, String)>,
}

impl StartupApp {
    #[must_use]
    pub fn new(mut model: StartupModel) -> Self {
        let page = if model.open_saves {
            StartupPage::Saves
        } else if model.open_settings {
            StartupPage::Settings
        } else if model.recovery_error.is_some() {
            StartupPage::Recovery
        } else if model.open_mods {
            StartupPage::Mods
        } else if model.new_game_only {
            match model.player_creation {
                StartupPlayerCreationView::Fixed => StartupPage::Main,
                StartupPlayerCreationView::Preset { .. } => StartupPage::Presets,
                StartupPlayerCreationView::Ugc { .. } => StartupPage::Form,
            }
        } else {
            StartupPage::Main
        };
        let form = match &model.player_creation {
            StartupPlayerCreationView::Ugc { form } => Some(StartupFormState::new(form)),
            StartupPlayerCreationView::Fixed | StartupPlayerCreationView::Preset { .. } => None,
        };
        let selected = if page == StartupPage::Main {
            usize::from(model.saves.is_empty())
        } else if page == StartupPage::Mods {
            mod_visual_order(&model.packages)
                .first()
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        let initial_enabled_mods = enabled_mod_keys(&model.packages);
        let notice = model.notice.clone();
        let initial_settings = model.setting_fields.clone();
        if let Some(draft) = model.settings_draft.take() {
            model.setting_fields = draft;
        }
        Self {
            delete_confirmation: None,
            settings_editor: None,
            initial_settings,
            model,
            page,
            selected,
            notice,
            form,
            initial_enabled_mods,
        }
    }

    fn return_to_main(&mut self) -> Option<StartupAction> {
        if self.model.recovery_error.is_some() {
            self.page = StartupPage::Recovery;
            self.selected = 0;
            self.notice = None;
            None
        } else if self.model.new_game_only {
            Some(StartupAction::Quit)
        } else {
            self.page = StartupPage::Main;
            self.selected = usize::from(self.model.saves.is_empty());
            self.notice = None;
            None
        }
    }
}

pub fn run_startup(model: StartupModel, config: TuiConfig) -> Result<StartupAction, TuiError> {
    if model.new_game_only
        && model.recovery_error.is_none()
        && !model.open_mods
        && !model.open_settings
        && matches!(&model.player_creation, StartupPlayerCreationView::Fixed)
    {
        return Ok(StartupAction::NewGame(StartupPlayerSelection::Fixed));
    }
    let mut terminal = TuiTerminal::open()?;
    terminal.run_startup(model, config)
}

impl TuiTerminal {
    pub fn run_startup(
        &mut self,
        model: StartupModel,
        config: TuiConfig,
    ) -> Result<StartupAction, TuiError> {
        if model.new_game_only
            && model.recovery_error.is_none()
            && !model.open_mods
            && !model.open_settings
            && matches!(&model.player_creation, StartupPlayerCreationView::Fixed)
        {
            self.show_loading(&model.world_name)?;
            return Ok(StartupAction::NewGame(StartupPlayerSelection::Fixed));
        }
        let config = config.validate()?;
        self.terminal.clear()?;
        let mut app = StartupApp::new(model);
        loop {
            self.terminal
                .draw(|frame| render_startup(frame, &mut app))?;
            if !event::poll(config.event_poll_interval)? {
                continue;
            }
            match event::read()? {
                Event::Key(key) => {
                    if let Some(action) = handle_startup_key(&mut app, key) {
                        if action != StartupAction::Quit
                            && !matches!(
                                &action,
                                StartupAction::BackToLauncher
                                    | StartupAction::DeleteSave { .. }
                                    | StartupAction::ApplyMods { .. }
                                    | StartupAction::ApplySettings { .. }
                            )
                        {
                            self.show_loading(&app.model.world_name)?;
                        }
                        return Ok(action);
                    }
                }
                Event::Paste(text) => handle_startup_paste(&mut app, &text),
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
            }
        }
    }
}

pub fn handle_startup_key(app: &mut StartupApp, key: KeyEvent) -> Option<StartupAction> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }
    if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(StartupAction::Quit);
    }
    if let Some((index, confirm)) = app.delete_confirmation.as_mut() {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Backspace => app.delete_confirmation = None,
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => *confirm = !*confirm,
            KeyCode::Enter => {
                let action = (*confirm).then_some(StartupAction::DeleteSave { index: *index });
                app.delete_confirmation = None;
                return action;
            }
            _ => {}
        }
        return None;
    }
    match app.page {
        StartupPage::Recovery => match key.code {
            KeyCode::Up => {
                app.selected = (app.selected + 3) % 4;
                None
            }
            KeyCode::Down | KeyCode::Tab => {
                app.selected = (app.selected + 1) % 4;
                None
            }
            KeyCode::Esc => Some(StartupAction::BackToLauncher),
            KeyCode::Enter => match app.selected {
                0 => Some(StartupAction::RetryStartup),
                1 => {
                    app.page = StartupPage::Settings;
                    app.selected = 0;
                    None
                }
                2 => Some(StartupAction::BackToLauncher),
                _ => Some(StartupAction::Quit),
            },
            _ => None,
        },
        StartupPage::Main => handle_main_key(app, key),
        StartupPage::Saves => handle_saves_key(app, key),
        StartupPage::Mods => handle_mods_key(app, key),
        StartupPage::Settings => handle_settings_key(app, key),
        StartupPage::Presets => handle_presets_key(app, key),
        StartupPage::Form => handle_form_key(app, key),
    }
}

fn handle_main_key(app: &mut StartupApp, key: KeyEvent) -> Option<StartupAction> {
    if app.model.new_game_only {
        return Some(StartupAction::NewGame(StartupPlayerSelection::Fixed));
    }
    match key.code {
        KeyCode::Up => app.selected = previous_main_entry(app, app.selected),
        KeyCode::Down | KeyCode::Tab => app.selected = next_main_entry(app, app.selected),
        KeyCode::Enter => match app.selected {
            0 if !app.model.saves.is_empty() => return Some(StartupAction::OpenSave { index: 0 }),
            1 => return enter_new_game(app),
            2 => {
                app.page = StartupPage::Saves;
                app.selected = 0;
            }
            3 => {
                app.page = StartupPage::Mods;
                app.selected = 0;
            }
            4 => {
                app.page = StartupPage::Settings;
                app.selected = 0;
            }
            5 => return Some(StartupAction::Quit),
            _ => {}
        },
        KeyCode::Esc => return Some(StartupAction::Quit),
        _ => {}
    }
    None
}

fn next_main_entry(app: &StartupApp, current: usize) -> usize {
    (1..=6)
        .map(|offset| (current + offset) % 6)
        .find(|index| main_entry_enabled(app, *index))
        .unwrap_or(current)
}

fn previous_main_entry(app: &StartupApp, current: usize) -> usize {
    (1..=6)
        .map(|offset| (current + 6 - offset) % 6)
        .find(|index| main_entry_enabled(app, *index))
        .unwrap_or(current)
}

fn main_entry_enabled(app: &StartupApp, index: usize) -> bool {
    index != 0 || !app.model.saves.is_empty()
}

fn enter_new_game(app: &mut StartupApp) -> Option<StartupAction> {
    match &app.model.player_creation {
        StartupPlayerCreationView::Fixed => {
            Some(StartupAction::NewGame(StartupPlayerSelection::Fixed))
        }
        StartupPlayerCreationView::Preset { .. } => {
            app.page = StartupPage::Presets;
            app.selected = 0;
            None
        }
        StartupPlayerCreationView::Ugc { .. } => {
            app.page = StartupPage::Form;
            app.selected = 0;
            None
        }
    }
}

fn handle_saves_key(app: &mut StartupApp, key: KeyEvent) -> Option<StartupAction> {
    match key.code {
        KeyCode::Delete | KeyCode::Char('d' | 'D')
            if app.model.saves.get(app.selected).is_some() =>
        {
            app.delete_confirmation = Some((app.selected, false));
        }
        KeyCode::Esc | KeyCode::Backspace => return app.return_to_main(),
        KeyCode::Up => app.selected = app.selected.saturating_sub(1),
        KeyCode::Down => {
            app.selected = app
                .selected
                .saturating_add(1)
                .min(app.model.saves.len().saturating_sub(1));
        }
        KeyCode::Enter if !app.model.saves.is_empty() => {
            return Some(StartupAction::OpenSave {
                index: app.selected,
            });
        }
        _ => {}
    }
    None
}

fn handle_settings_key(app: &mut StartupApp, key: KeyEvent) -> Option<StartupAction> {
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        commit_setting_edit(app);
        return Some(StartupAction::ApplySettings {
            fields: app.model.setting_fields.clone(),
        });
    }
    if let Some(editor) = app.settings_editor.as_mut() {
        match key.code {
            KeyCode::Esc => app.settings_editor = None,
            KeyCode::Enter => commit_setting_edit(app),
            KeyCode::Left => editor.move_left(),
            KeyCode::Right => editor.move_right(),
            KeyCode::Home => editor.move_home(),
            KeyCode::End => editor.move_end(),
            KeyCode::Backspace => {
                editor.backspace();
            }
            KeyCode::Delete => {
                editor.delete();
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && editor.insert(&character.to_string()).is_err() =>
            {
                app.notice = Some("Setting exceeds the input limit.".to_owned());
            }
            _ => {}
        }
        return None;
    }
    let count = app.model.setting_fields.len();
    match key.code {
        KeyCode::Esc => {
            app.model.setting_fields.clone_from(&app.initial_settings);
            return app.return_to_main();
        }
        KeyCode::Up | KeyCode::BackTab => app.selected = app.selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => {
            app.selected = (app.selected + 1).min(count.saturating_sub(1))
        }
        KeyCode::PageUp => app.selected = app.selected.saturating_sub(10),
        KeyCode::PageDown => app.selected = (app.selected + 10).min(count.saturating_sub(1)),
        KeyCode::Enter => {
            if let Some(field) = app.model.setting_fields.get(app.selected) {
                match InputEditor::with_text(&field.value) {
                    Ok(editor) => app.settings_editor = Some(editor),
                    Err(_) => app.notice = Some("Setting exceeds the input limit.".to_owned()),
                }
            }
        }
        _ => {}
    }
    None
}

fn commit_setting_edit(app: &mut StartupApp) {
    if let Some(editor) = app.settings_editor.take()
        && let Some(field) = app.model.setting_fields.get_mut(app.selected)
    {
        field.value = editor.text().to_owned();
    }
}

fn handle_mods_key(app: &mut StartupApp, key: KeyEvent) -> Option<StartupAction> {
    let package_count = app.model.packages.mods.len();
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => {
            restore_initial_mod_selection(app);
            return app.return_to_main();
        }
        KeyCode::Up => {
            move_mod_selection(app, 1, false);
            app.notice = None;
        }
        KeyCode::Down => {
            move_mod_selection(app, 1, true);
            app.notice = None;
        }
        KeyCode::PageUp => {
            move_mod_selection(app, 5, false);
            app.notice = None;
        }
        KeyCode::PageDown => {
            move_mod_selection(app, 5, true);
            app.notice = None;
        }
        KeyCode::Char(' ') if package_count > 0 => {
            toggle_selected_mod(app);
            app.notice = None;
        }
        KeyCode::Enter => {
            let enabled = app
                .model
                .packages
                .mods
                .iter()
                .filter(|package| package.status == ModPackageStatus::Enabled)
                .cloned()
                .collect();
            return Some(StartupAction::ApplyMods { enabled });
        }
        _ => {}
    }
    None
}

fn move_mod_selection(app: &mut StartupApp, distance: usize, forward: bool) {
    let order = mod_visual_order(&app.model.packages);
    let Some(position) = order.iter().position(|index| *index == app.selected) else {
        app.selected = order.first().copied().unwrap_or(0);
        return;
    };
    let next = if forward {
        position
            .saturating_add(distance)
            .min(order.len().saturating_sub(1))
    } else {
        position.saturating_sub(distance)
    };
    app.selected = order[next];
}

fn mod_visual_order(packages: &PackageCatalogView) -> Vec<usize> {
    [ModPackageStatus::Enabled, ModPackageStatus::Installed]
        .into_iter()
        .flat_map(|status| {
            packages
                .mods
                .iter()
                .enumerate()
                .filter_map(move |(index, package)| (package.status == status).then_some(index))
        })
        .collect()
}

fn toggle_selected_mod(app: &mut StartupApp) {
    let Some(selected) = app.model.packages.mods.get(app.selected) else {
        return;
    };
    let mod_id = selected.mod_id.clone();
    let enable = selected.status == ModPackageStatus::Installed;
    for (index, package) in app.model.packages.mods.iter_mut().enumerate() {
        if index == app.selected {
            package.status = if enable {
                ModPackageStatus::Enabled
            } else {
                ModPackageStatus::Installed
            };
        } else if enable && package.mod_id == mod_id {
            package.status = ModPackageStatus::Installed;
        }
    }
}

fn restore_initial_mod_selection(app: &mut StartupApp) {
    for package in &mut app.model.packages.mods {
        package.status = if app.initial_enabled_mods.contains(&mod_package_key(package)) {
            ModPackageStatus::Enabled
        } else {
            ModPackageStatus::Installed
        };
    }
}

fn enabled_mod_keys(packages: &PackageCatalogView) -> BTreeSet<(ModId, String)> {
    packages
        .mods
        .iter()
        .filter(|package| package.status == ModPackageStatus::Enabled)
        .map(mod_package_key)
        .collect()
}

fn mod_package_key(package: &loreloom_core::ModPackageView) -> (ModId, String) {
    (package.mod_id.clone(), package.version.to_string())
}

fn handle_presets_key(app: &mut StartupApp, key: KeyEvent) -> Option<StartupAction> {
    let StartupPlayerCreationView::Preset { characters } = &app.model.player_creation else {
        return app.return_to_main();
    };
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => return app.return_to_main(),
        KeyCode::Up => app.selected = app.selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => {
            app.selected = app
                .selected
                .saturating_add(1)
                .min(characters.len().saturating_sub(1));
        }
        KeyCode::Enter => {
            if let Some(character) = characters.get(app.selected) {
                return Some(StartupAction::NewGame(StartupPlayerSelection::Preset {
                    character_id: character.character_id.clone(),
                }));
            }
        }
        _ => {}
    }
    None
}

fn handle_form_key(app: &mut StartupApp, key: KeyEvent) -> Option<StartupAction> {
    if matches!(key.code, KeyCode::Esc) {
        return app.return_to_main();
    }
    let StartupPlayerCreationView::Ugc { form } = &app.model.player_creation else {
        return app.return_to_main();
    };
    let Some(state) = app.form.as_mut() else {
        return app.return_to_main();
    };
    if form.fields.is_empty() {
        app.notice = Some("form_has_no_fields".to_owned());
        return None;
    }
    let current = state.current;
    let field = &form.fields[current];
    let mut validate_after_edit = false;
    match key.code {
        KeyCode::BackTab | KeyCode::Up => {
            let _ = validate_current_form_field(form, state);
            state.select(current.saturating_sub(1));
            app.notice = state.current_validation_notice();
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let _ = validate_current_form_field(form, state);
            state.select(current.saturating_sub(1));
            app.notice = state.current_validation_notice();
        }
        KeyCode::Tab | KeyCode::Down => {
            let _ = validate_current_form_field(form, state);
            state.select((current + 1).min(form.fields.len() - 1));
            app.notice = state.current_validation_notice();
        }
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::ALT)
                && matches!(field.kind, StartupFieldKind::LongText { .. }) =>
        {
            insert_form_text(state, field, "\n");
            validate_after_edit = true;
        }
        KeyCode::Char('j')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(field.kind, StartupFieldKind::LongText { .. }) =>
        {
            insert_form_text(state, field, "\n");
            validate_after_edit = true;
        }
        KeyCode::Enter => {
            if let Some(error) = validate_current_form_field(form, state) {
                app.notice = Some(error.notice);
                return None;
            }
            if current + 1 < form.fields.len() {
                state.select(current + 1);
                app.notice = state.current_validation_notice();
            } else {
                match submit_form(form, state) {
                    Ok(submission) => {
                        return Some(StartupAction::NewGame(StartupPlayerSelection::Ugc(
                            submission,
                        )));
                    }
                    Err(error) => {
                        state.select(error.field_index);
                        app.notice = Some(error.notice);
                    }
                }
            }
        }
        KeyCode::Left => {
            if editor_text(&state.values[current]).is_some() {
                state.editor.move_left();
            } else {
                adjust_form_value(state, field, -1);
                validate_after_edit = true;
            }
        }
        KeyCode::Right => {
            if editor_text(&state.values[current]).is_some() {
                state.editor.move_right();
            } else {
                adjust_form_value(state, field, 1);
                validate_after_edit = true;
            }
        }
        KeyCode::Char(' ') => {
            toggle_form_value(state, field);
            validate_after_edit = true;
        }
        KeyCode::Home => state.editor.move_home(),
        KeyCode::End => state.editor.move_end(),
        KeyCode::Backspace => {
            state.editor.backspace();
            validate_after_edit = true;
        }
        KeyCode::Delete => {
            state.editor.delete();
            validate_after_edit = true;
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            insert_form_text(state, field, &character.to_string());
            validate_after_edit = true;
        }
        _ => {}
    }
    if validate_after_edit {
        app.notice = validate_current_form_field(form, state).map(|error| error.notice);
    }
    None
}

fn handle_startup_paste(app: &mut StartupApp, text: &str) {
    if app.page == StartupPage::Settings {
        if let Some(editor) = app.settings_editor.as_mut() {
            if text.chars().any(char::is_control) {
                app.notice = Some("Paste a single-line setting value.".to_owned());
            } else if editor.insert(text).is_err() {
                app.notice = Some("Setting exceeds the input limit.".to_owned());
            }
        }
        return;
    }
    if app.page != StartupPage::Form {
        return;
    }
    let StartupPlayerCreationView::Ugc { form } = &app.model.player_creation else {
        return;
    };
    let Some(state) = app.form.as_mut() else {
        return;
    };
    if let Some(field) = form.fields.get(state.current) {
        insert_form_text(state, field, text);
        app.notice = validate_current_form_field(form, state).map(|error| error.notice);
    }
}

fn insert_form_text(state: &mut StartupFormState, field: &StartupFieldView, text: &str) {
    if !matches!(
        field.kind,
        StartupFieldKind::Text { .. }
            | StartupFieldKind::LongText { .. }
            | StartupFieldKind::Integer { .. }
            | StartupFieldKind::Number { .. }
    ) {
        return;
    }
    let previous = state.editor.clone();
    if state.editor.insert(text).is_err() || state.editor.text().len() > field_maximum_bytes(field)
    {
        state.editor = previous;
    }
}

fn field_maximum_bytes(field: &StartupFieldView) -> usize {
    match field.kind {
        StartupFieldKind::Text { maximum_bytes, .. }
        | StartupFieldKind::LongText { maximum_bytes, .. } => maximum_bytes as usize,
        StartupFieldKind::Integer { .. } | StartupFieldKind::Number { .. } => 64,
        StartupFieldKind::Boolean { .. }
        | StartupFieldKind::SingleChoice { .. }
        | StartupFieldKind::MultiChoice { .. } => 0,
    }
}

fn adjust_form_value(state: &mut StartupFormState, field: &StartupFieldView, direction: i8) {
    let current = state.current;
    match (&field.kind, &mut state.values[current]) {
        (
            StartupFieldKind::Integer {
                minimum, maximum, ..
            },
            FormValueState::Integer(raw),
        ) => {
            let value = raw.parse::<i64>().unwrap_or(*minimum);
            let adjusted = if direction < 0 {
                value.saturating_sub(1)
            } else {
                value.saturating_add(1)
            }
            .clamp(*minimum, *maximum);
            *raw = adjusted.to_string();
            state.load_editor();
        }
        (
            StartupFieldKind::Number {
                minimum, maximum, ..
            },
            FormValueState::Number(raw),
        ) => {
            let value = parse_fixed(raw).unwrap_or(*minimum);
            let adjusted = if direction < 0 {
                value.checked_sub(Fixed::ONE)
            } else {
                value.checked_add(Fixed::ONE)
            }
            .unwrap_or(value)
            .clamp(*minimum, *maximum);
            *raw = format_fixed(adjusted);
            state.load_editor();
        }
        (StartupFieldKind::Boolean { .. }, FormValueState::Boolean(value)) => *value = !*value,
        (
            StartupFieldKind::SingleChoice { options, .. },
            FormValueState::SingleChoice(selected),
        ) => {
            if options.is_empty() {
                return;
            }
            let position = selected
                .as_ref()
                .and_then(|selected| options.iter().position(|option| &option.value == selected))
                .unwrap_or(0);
            let next = if direction < 0 {
                position.checked_sub(1).unwrap_or(options.len() - 1)
            } else {
                (position + 1) % options.len()
            };
            *selected = Some(options[next].value.clone());
            state.option_cursors[current] = next;
        }
        (StartupFieldKind::MultiChoice { options, .. }, FormValueState::MultiChoice(_)) => {
            if options.is_empty() {
                return;
            }
            let position = state.option_cursors[current];
            state.option_cursors[current] = if direction < 0 {
                position.checked_sub(1).unwrap_or(options.len() - 1)
            } else {
                (position + 1) % options.len()
            };
        }
        (StartupFieldKind::Text { .. } | StartupFieldKind::LongText { .. }, _) => {
            if direction < 0 {
                state.editor.move_up();
            } else {
                state.editor.move_down();
            }
        }
        _ => {}
    }
}

fn toggle_form_value(state: &mut StartupFormState, field: &StartupFieldView) {
    let current = state.current;
    match (&field.kind, &mut state.values[current]) {
        (StartupFieldKind::Boolean { .. }, FormValueState::Boolean(value)) => *value = !*value,
        (StartupFieldKind::MultiChoice { options, .. }, FormValueState::MultiChoice(selected)) => {
            if let Some(option) = options.get(state.option_cursors[current])
                && !selected.remove(&option.value)
            {
                selected.insert(option.value.clone());
            }
        }
        _ => insert_form_text(state, field, " "),
    }
}

fn submit_form(
    form: &StartupFormView,
    state: &mut StartupFormState,
) -> Result<StartupFormSubmission, FormValidationError> {
    state.store_editor();
    let mut values = Vec::with_capacity(form.fields.len());
    for (field_index, (field, value)) in form.fields.iter().zip(&state.values).enumerate() {
        match validate_form_field(field_index, field, value) {
            Ok(Some(value)) => {
                state.validation_errors[field_index] = None;
                values.push((field.field_id.clone(), value));
            }
            Ok(None) => state.validation_errors[field_index] = None,
            Err(error) => {
                state.validation_errors[field_index] = Some(error.notice.clone());
                return Err(error);
            }
        }
    }
    Ok(StartupFormSubmission {
        form_id: form.form_id.clone(),
        values,
    })
}

fn validate_current_form_field(
    form: &StartupFormView,
    state: &mut StartupFormState,
) -> Option<FormValidationError> {
    state.store_editor();
    let field_index = state.current;
    let error = validate_form_field(
        field_index,
        &form.fields[field_index],
        &state.values[field_index],
    )
    .err();
    state.validation_errors[field_index] = error.as_ref().map(|error| error.notice.clone());
    error
}

fn validate_form_field(
    field_index: usize,
    field: &StartupFieldView,
    value: &FormValueState,
) -> Result<Option<StartupFieldValue>, FormValidationError> {
    let invalid = |expectation: String| FormValidationError {
        field_index,
        notice: format!("{} · {expectation}", field.display_name),
    };
    match (&field.kind, value) {
        (
            StartupFieldKind::Text {
                minimum_bytes,
                maximum_bytes,
                ..
            }
            | StartupFieldKind::LongText {
                minimum_bytes,
                maximum_bytes,
                ..
            },
            FormValueState::Text(value),
        ) => {
            if !field.required && value.is_empty() {
                return Ok(None);
            }
            if value.len() < *minimum_bytes as usize || value.len() > *maximum_bytes as usize {
                return Err(invalid(format!(
                    "expected {minimum_bytes}..={maximum_bytes} UTF-8 bytes"
                )));
            }
            Ok(Some(StartupFieldValue::Text(value.clone())))
        }
        (
            StartupFieldKind::Integer {
                minimum, maximum, ..
            },
            FormValueState::Integer(raw),
        ) => {
            if !field.required && raw.is_empty() {
                return Ok(None);
            }
            let value = raw
                .parse::<i64>()
                .map_err(|_| invalid(format!("expected an integer from {minimum} to {maximum}")))?;
            if value < *minimum || value > *maximum {
                return Err(invalid(format!(
                    "expected an integer from {minimum} to {maximum}"
                )));
            }
            Ok(Some(StartupFieldValue::Integer(value)))
        }
        (
            StartupFieldKind::Number {
                minimum, maximum, ..
            },
            FormValueState::Number(raw),
        ) => {
            if !field.required && raw.is_empty() {
                return Ok(None);
            }
            let expectation = || {
                format!(
                    "expected a number from {} to {} (up to 6 decimals)",
                    format_fixed(*minimum),
                    format_fixed(*maximum)
                )
            };
            let value = parse_fixed(raw).ok_or_else(|| invalid(expectation()))?;
            if value < *minimum || value > *maximum {
                return Err(invalid(expectation()));
            }
            Ok(Some(StartupFieldValue::Number(value)))
        }
        (StartupFieldKind::Boolean { .. }, FormValueState::Boolean(value)) => {
            Ok(Some(StartupFieldValue::Boolean(*value)))
        }
        (
            StartupFieldKind::SingleChoice { options, .. },
            FormValueState::SingleChoice(selected),
        ) => match selected {
            Some(value) if options.iter().any(|option| &option.value == value) => {
                Ok(Some(StartupFieldValue::SingleChoice(value.clone())))
            }
            None if !field.required => Ok(None),
            Some(_) | None => Err(invalid("select one of the available options".to_owned())),
        },
        (
            StartupFieldKind::MultiChoice {
                minimum_selections,
                maximum_selections,
                options,
                ..
            },
            FormValueState::MultiChoice(selected),
        ) if selected.len() >= *minimum_selections as usize
            && selected.len() <= *maximum_selections as usize
            && selected
                .iter()
                .all(|value| options.iter().any(|option| &option.value == value)) =>
        {
            Ok(Some(StartupFieldValue::MultiChoice(selected.clone())))
        }
        (
            StartupFieldKind::MultiChoice {
                minimum_selections,
                maximum_selections,
                ..
            },
            _,
        ) => Err(invalid(format!(
            "select {minimum_selections}..={maximum_selections} options"
        ))),
        _ => Err(invalid("field type mismatch".to_owned())),
    }
}

fn parse_fixed(raw: &str) -> Option<Fixed> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let negative = raw.starts_with('-');
    let unsigned = raw.strip_prefix(['-', '+']).unwrap_or(raw);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if whole.is_empty()
        || fraction.len() > 6
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let whole = whole.parse::<i128>().ok()?;
    let fraction = if fraction.is_empty() {
        0_i128
    } else {
        fraction.parse::<i128>().ok()? * 10_i128.pow(6_u32.saturating_sub(fraction.len() as u32))
    };
    let micros = whole
        .checked_mul(i128::from(Fixed::SCALE))?
        .checked_add(fraction)?;
    let micros = if negative {
        micros.checked_neg()?
    } else {
        micros
    };
    i64::try_from(micros).ok().map(Fixed::from_micros)
}

pub fn render_startup(frame: &mut Frame<'_>, app: &mut StartupApp) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);
    render_startup_header(frame, app, rows[0]);
    match app.page {
        StartupPage::Recovery => render_recovery(frame, app, rows[1]),
        StartupPage::Main => render_main(frame, app, rows[1]),
        StartupPage::Saves => render_saves(frame, app, rows[1]),
        StartupPage::Mods => render_mods(frame, app, rows[1]),
        StartupPage::Settings => render_settings(frame, app, rows[1]),
        StartupPage::Presets => render_presets(frame, app, rows[1]),
        StartupPage::Form => render_form(frame, app, rows[1]),
    }
    render_startup_footer(frame, app, rows[2]);
    if let Some((index, confirm)) = app.delete_confirmation
        && let Some(save) = app.model.saves.get(index)
    {
        let width = area.width.min(64);
        let height = area.height.min(12);
        let popup = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("Delete {}?", save.display_name),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "This permanently deletes the save. This cannot be undone.",
                    Style::default().fg(MUTED),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        if confirm { "  Cancel  " } else { "[ Cancel ]" },
                        if confirm {
                            Style::default()
                        } else {
                            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                        },
                    ),
                    Span::raw("     "),
                    Span::styled(
                        if confirm { "[ Delete ]" } else { "  Delete  " },
                        if confirm {
                            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                ]),
                Line::from(Span::styled(
                    "←/→ or Tab: choose · Enter: select",
                    Style::default().fg(MUTED),
                )),
            ])
            .wrap(Wrap { trim: false })
            .block(
                Block::bordered()
                    .title(Span::styled(
                        " DELETE SAVE · Esc cancel ",
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ))
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(ACCENT))
                    .padding(ratatui::widgets::Padding::horizontal(1)),
            ),
            popup,
        );
    }
}

fn render_recovery(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let rows =
        Layout::vertical([Constraint::Min(1), Constraint::Length(5)]).split(inset(area, 2, 0));
    let mut lines = vec![
        Line::from(Span::styled(
            "Unable to start the game",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(app.model.recovery_error.clone().unwrap_or_default()),
        Line::from("Your game selection is kept. Fix the configuration, then retry."),
        Line::from(
            "Settings accepts env:NAME or file:/path references. Exporting in another shell cannot update this process.",
        ),
    ];
    if let Some(notice) = &app.notice {
        lines.push(Line::from(notice.clone()));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rows[0]);
    let actions = ["Retry", "Settings", "Back to launcher", "Quit"];
    frame.render_widget(
        Paragraph::new(
            actions
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    Line::from(Span::styled(
                        format!(
                            "{}{}",
                            if index == app.selected { "› " } else { "  " },
                            label
                        ),
                        Style::default().fg(if index == app.selected {
                            ACCENT
                        } else {
                            Color::White
                        }),
                    ))
                })
                .collect::<Vec<_>>(),
        ),
        rows[1],
    );
}

fn render_startup_header(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            "LORELOOM",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", app.model.world_name),
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(MUTED)),
        ),
        area,
    );
}

fn render_main(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let entries = [
        ("Continue", !app.model.saves.is_empty()),
        ("New Game", true),
        ("Saves", true),
        ("Mods", true),
        ("Settings", true),
        ("Quit", true),
    ];
    let mut lines = vec![
        Line::from(Span::styled(
            "Enter the world",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            &app.model.world_id,
            Style::default().fg(MUTED),
        )),
        Line::from(""),
    ];
    for (index, (label, enabled)) in entries.iter().enumerate() {
        let selected = index == app.selected;
        let marker = if selected { "› " } else { "  " };
        let style = if !enabled {
            Style::default().fg(MUTED)
        } else if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(format!("{marker}{label}"), style)));
    }
    if let Some(save) = app.model.saves.first() {
        lines.extend([
            Line::from(""),
            Line::from(Span::styled("MOST RECENT", Style::default().fg(MUTED))),
            Line::from(save.display_name.clone()),
            Line::from(Span::styled(
                save.detail.clone(),
                Style::default().fg(MUTED),
            )),
        ]);
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inset(area, 3, 2),
    );
}

fn render_saves(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let mut lines = vec![Line::from(Span::styled(
        "SAVES",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    ))];
    if app.model.saves.is_empty() {
        lines.push(Line::from("No saves yet. Start a New Game to create one."));
    }
    if let Some(notice) = &app.notice {
        lines.push(Line::from(notice.clone()));
    }
    let visible = usize::from(area.height.saturating_sub(2))
        .saturating_sub(lines.len())
        .max(1);
    let offset = app.selected.saturating_sub(visible.saturating_sub(1));
    for (index, save) in app
        .model
        .saves
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
    {
        lines.push(Line::from(vec![
            Span::styled(
                if index == app.selected { "› " } else { "  " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                save.display_name.clone(),
                if index == app.selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
            Span::styled(format!("  {}", save.detail), Style::default().fg(MUTED)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inset(area, 2, 1));
}

fn render_mods(frame: &mut Frame<'_>, app: &mut StartupApp, area: Rect) {
    let body = inset(area, 2, 1);
    let catalog = &app.model.packages;
    let enabled = catalog
        .mods
        .iter()
        .enumerate()
        .filter(|(_, package)| package.status == ModPackageStatus::Enabled)
        .collect::<Vec<_>>();
    let installed = catalog
        .mods
        .iter()
        .enumerate()
        .filter(|(_, package)| package.status == ModPackageStatus::Installed)
        .collect::<Vec<_>>();
    let mut selected_row = None;
    let mut lines = vec![
        Line::from(Span::styled(
            "WORLD",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("◆ ", Style::default().fg(ACCENT)),
            Span::styled(
                catalog.world.world_id.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            format!("  v{} · main world", catalog.world.version),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("ENABLED ({})", enabled.len()),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
    ];
    if enabled.is_empty() {
        lines.push(Line::from(Span::styled(
            "No enabled extension Mods.",
            Style::default().fg(MUTED),
        )));
    } else {
        for (index, package) in enabled {
            if index == app.selected {
                selected_row = Some(lines.len());
            }
            push_mod_package(&mut lines, package, true, Some(index == app.selected));
        }
    }
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            format!("INSTALLED, NOT ENABLED ({})", installed.len()),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
    ]);
    if installed.is_empty() {
        lines.push(Line::from(Span::styled(
            "No valid inactive Mods found in mods/.",
            Style::default().fg(MUTED),
        )));
    } else {
        for (index, package) in installed {
            if index == app.selected {
                selected_row = Some(lines.len());
            }
            push_mod_package(&mut lines, package, false, Some(index == app.selected));
        }
    }
    if catalog.unavailable_installed > 0 {
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                format!(
                    "! {} installed candidate(s) unavailable",
                    catalog.unavailable_installed
                ),
                Style::default().fg(Color::Yellow),
            )),
        ]);
    }
    if let Some(notice) = &app.notice {
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                notice.clone(),
                Style::default().fg(Color::Yellow),
            )),
        ]);
    }
    let page_rows = usize::from(body.height.max(1));
    let scroll = selected_row
        .map(|row| row.saturating_sub(page_rows.saturating_sub(1)))
        .unwrap_or(0);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
            .wrap(Wrap { trim: false }),
        body,
    );
}

fn render_settings(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(1),
        Constraint::Length(4),
    ])
    .split(inset(area, 2, 1));
    let mut heading = vec![Line::from(Span::styled(
        "SETTINGS",
        Style::default().fg(ACCENT),
    ))];
    heading.extend(app.model.settings.iter().cloned().map(Line::from));
    frame.render_widget(Paragraph::new(heading), rows[0]);
    let visible = usize::from(rows[1].height).max(1);
    let start = app.selected.saturating_sub(visible - 1);
    let lines = app
        .model
        .setting_fields
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(index, field)| {
            let selected = index == app.selected;
            let value = if selected {
                app.settings_editor
                    .as_ref()
                    .map_or_else(|| field.value.clone(), InputEditor::text_with_cursor)
            } else {
                field.value.clone()
            };
            Line::from(Span::styled(
                format!(
                    "{}{}  =  {}",
                    if selected { "› " } else { "  " },
                    field.key,
                    value
                ),
                Style::default().fg(if selected { ACCENT } else { Color::White }),
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), rows[1]);
    if let Some(field) = app.model.setting_fields.get(app.selected) {
        let value = app
            .settings_editor
            .as_ref()
            .map_or_else(|| field.value.clone(), InputEditor::text_with_cursor);
        frame.render_widget(
            Paragraph::new(
                app.notice
                    .clone()
                    .unwrap_or_else(|| format!("{}\n{}", field.help, value)),
            )
            .wrap(Wrap { trim: false }),
            rows[2],
        );
    }
}

fn render_presets(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let StartupPlayerCreationView::Preset { characters } = &app.model.player_creation else {
        return;
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(inset(area, 1, 0));
    let list = characters
        .iter()
        .enumerate()
        .map(|(index, character)| {
            Line::from(Span::styled(
                format!(
                    "{}{}",
                    if index == app.selected { "› " } else { "  " },
                    character.display_name
                ),
                if index == app.selected {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(list).block(
            Block::default()
                .title(" CHARACTERS ")
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(MUTED)),
        ),
        columns[0],
    );
    if let Some(character) = characters.get(app.selected) {
        let mut details = vec![
            Line::from(Span::styled(
                character.display_name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(character.summary.clone()),
            Line::from(""),
        ];
        details.extend(character.details.iter().cloned().map(Line::from));
        frame.render_widget(
            Paragraph::new(details).wrap(Wrap { trim: false }),
            inset(columns[1], 2, 1),
        );
    }
}

fn render_form(frame: &mut Frame<'_>, app: &mut StartupApp, area: Rect) {
    let StartupPlayerCreationView::Ugc { form } = &app.model.player_creation else {
        return;
    };
    let Some(state) = app.form.as_mut() else {
        return;
    };
    state.store_editor();
    let columns = if area.width >= 80 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(4)])
            .split(area)
    };
    let mut preview = vec![
        Line::from(Span::styled(
            "CHARACTER CARD",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            form.display_name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(form.description.clone()),
        Line::from(""),
        Line::from(Span::styled(
            format!("Field {} of {}", state.current + 1, form.fields.len()),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
    ];
    for (field, value) in form.fields.iter().zip(&state.values) {
        preview.push(Line::from(vec![
            Span::styled(
                format!("{}  ", field.display_name),
                Style::default().fg(MUTED),
            ),
            Span::raw(preview_form_value(field, value)),
        ]));
    }
    frame.render_widget(
        Paragraph::new(preview)
            .block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_type(BorderType::Plain)
                    .border_style(Style::default().fg(MUTED)),
            )
            .wrap(Wrap { trim: false }),
        inset(columns[0], 2, 1),
    );
    let mut fields = Vec::new();
    for (index, (field, value)) in form.fields.iter().zip(&state.values).enumerate() {
        let selected = index == state.current;
        let invalid = state.validation_errors[index].is_some();
        fields.push(Line::from(vec![
            Span::styled(
                if selected {
                    "› "
                } else if invalid {
                    "! "
                } else {
                    "  "
                },
                Style::default().fg(if invalid { Color::Red } else { ACCENT }),
            ),
            Span::styled(
                format!(
                    "{}{}",
                    field.display_name,
                    if field.required { " *" } else { "" }
                ),
                if selected {
                    Style::default()
                        .fg(if invalid { Color::Red } else { Color::Reset })
                        .add_modifier(Modifier::BOLD)
                } else if invalid {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default()
                },
            ),
            Span::styled(
                format!("  {}", display_form_value(field, value, state, index)),
                Style::default().fg(if invalid {
                    Color::Red
                } else if selected {
                    ACCENT
                } else {
                    MUTED
                }),
            ),
        ]));
    }
    if let Some(field) = form.fields.get(state.current)
        && let Some(description) = &field.description
    {
        fields.extend([
            Line::from(""),
            Line::from(Span::styled(
                description.clone(),
                Style::default().fg(MUTED),
            )),
        ]);
    }
    if let Some(notice) = &app.notice {
        fields.extend([
            Line::from(""),
            Line::from(Span::styled(
                notice.clone(),
                Style::default().fg(Color::Red),
            )),
        ]);
    }
    frame.render_widget(
        Paragraph::new(fields).wrap(Wrap { trim: false }),
        inset(columns[1], 2, 1),
    );
}

fn preview_form_value(field: &StartupFieldView, value: &FormValueState) -> String {
    match value {
        FormValueState::Text(text)
        | FormValueState::Integer(text)
        | FormValueState::Number(text) => {
            if text.is_empty() {
                "—".to_owned()
            } else {
                text.replace('\n', " / ")
            }
        }
        FormValueState::Boolean(value) => if *value { "Yes" } else { "No" }.to_owned(),
        FormValueState::SingleChoice(selected) => selected
            .as_ref()
            .and_then(|selected| match &field.kind {
                StartupFieldKind::SingleChoice { options, .. } => options
                    .iter()
                    .find(|option| &option.value == selected)
                    .map(|option| option.display_name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "—".to_owned()),
        FormValueState::MultiChoice(selected) => match &field.kind {
            StartupFieldKind::MultiChoice { options, .. } => {
                let selected = options
                    .iter()
                    .filter(|option| selected.contains(&option.value))
                    .map(|option| option.display_name.as_str())
                    .collect::<Vec<_>>();
                if selected.is_empty() {
                    "—".to_owned()
                } else {
                    selected.join(", ")
                }
            }
            _ => "—".to_owned(),
        },
    }
}

fn display_form_value(
    field: &StartupFieldView,
    value: &FormValueState,
    state: &StartupFormState,
    index: usize,
) -> String {
    match value {
        FormValueState::Text(text)
        | FormValueState::Integer(text)
        | FormValueState::Number(text) => {
            if index == state.current {
                state.editor.text_with_cursor().replace('\n', " ↵ ")
            } else if text.is_empty() {
                "—".to_owned()
            } else {
                text.replace('\n', " ↵ ")
            }
        }
        FormValueState::Boolean(value) => if *value { "Yes" } else { "No" }.to_owned(),
        FormValueState::SingleChoice(selected) => selected
            .as_ref()
            .and_then(|selected| match &field.kind {
                StartupFieldKind::SingleChoice { options, .. } => options
                    .iter()
                    .find(|option| &option.value == selected)
                    .map(|option| option.display_name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "—".to_owned()),
        FormValueState::MultiChoice(selected) => match &field.kind {
            StartupFieldKind::MultiChoice { options, .. } => options
                .iter()
                .enumerate()
                .map(|(option_index, option)| {
                    let cursor =
                        if index == state.current && option_index == state.option_cursors[index] {
                            "›"
                        } else {
                            " "
                        };
                    let mark = if selected.contains(&option.value) {
                        "×"
                    } else {
                        " "
                    };
                    format!("{cursor}[{mark}] {}", option.display_name)
                })
                .collect::<Vec<_>>()
                .join("  "),
            _ => "—".to_owned(),
        },
    }
}

fn render_startup_footer(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let hint = match app.page {
        StartupPage::Recovery => "↑↓ select  Enter open  Esc back to launcher",
        StartupPage::Main => "↑↓ select  Enter open  Esc quit",
        StartupPage::Saves => "↑↓ select  Enter load  D/Delete delete  Esc back",
        StartupPage::Presets => "↑↓ select  Enter confirm  Esc back",
        StartupPage::Mods => "↑↓ select  Space toggle  Enter apply  Esc cancel",
        StartupPage::Settings => "↑↓ select  Enter edit/accept  Ctrl+S save  Esc cancel",
        StartupPage::Form => "↑↓/Tab field  ←→ choose  Space toggle  Enter next/confirm  Esc back",
    };
    frame.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(MUTED))).alignment(Alignment::Center),
        area,
    );
}

fn inset(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(horizontal),
        area.y.saturating_add(vertical),
        area.width.saturating_sub(horizontal.saturating_mul(2)),
        area.height.saturating_sub(vertical.saturating_mul(2)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use loreloom_core::{ModPackageView, PackageCatalogView, PackageContentView, WorldPackageView};
    use ratatui::{Terminal, backend::TestBackend};

    fn id(kind: &str, key: &str) -> ContentDefinitionId {
        format!("games.loreloom.test:{kind}/{key}")
            .parse()
            .expect("definition ID")
    }

    fn fixed_model() -> StartupModel {
        StartupModel {
            world_name: "Rainbound Inn".to_owned(),
            world_id: "games.loreloom.test".to_owned(),
            saves: Vec::new(),
            packages: PackageCatalogView {
                world: WorldPackageView {
                    world_id: "games.loreloom.test".parse().expect("world ID"),
                    version: "1.0.0".parse().expect("version"),
                },
                mods: Vec::new(),
                unavailable_installed: 0,
            },
            settings: vec!["Configuration  loreloom.toml".to_owned()],
            setting_fields: vec![],
            settings_draft: None,
            open_settings: false,
            open_saves: false,
            recovery_error: None,
            player_creation: StartupPlayerCreationView::Fixed,
            new_game_only: false,
            open_mods: false,
            notice: None,
        }
    }

    fn settings_model() -> StartupModel {
        let mut model = fixed_model();
        model.setting_fields = vec![StartupSettingView {
            key: "narrator.model".to_owned(),
            value: "original".to_owned(),
            help: "Model name".to_owned(),
        }];
        model
    }

    fn press(app: &mut StartupApp, code: KeyCode) -> Option<StartupAction> {
        handle_startup_key(app, KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn settings_edit_paste_save_and_cancel_from_launcher() {
        let mut app = StartupApp::new(settings_model());
        app.selected = 4;
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.page, StartupPage::Settings);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::End);
        handle_startup_paste(&mut app, "-新");
        let action = handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        let Some(StartupAction::ApplySettings { fields }) = action else {
            panic!("save action");
        };
        assert_eq!(fields[0].value, "original-新");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.page, StartupPage::Main);
        app.selected = 4;
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.model.setting_fields[0].value, "original");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.model.setting_fields[0].value, "original");
    }

    #[test]
    fn settings_failed_save_keeps_draft_but_cancel_restores_saved_values() {
        let mut model = settings_model();
        let mut draft = model.setting_fields.clone();
        draft[0].value = "rejected".to_owned();
        model.settings_draft = Some(draft);
        model.open_settings = true;
        model.notice = Some("Settings could not be saved".to_owned());
        let mut app = StartupApp::new(model);
        assert_eq!(app.model.setting_fields[0].value, "rejected");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.model.setting_fields[0].value, "original");
    }

    #[test]
    fn settings_render_selected_field_after_scrolling_in_small_terminal() {
        let mut model = settings_model();
        model.open_settings = true;
        model
            .setting_fields
            .extend((0..60).map(|index| StartupSettingView {
                key: format!("budget.{index}"),
                value: "12".to_owned(),
                help: "Integer".to_owned(),
            }));
        let mut app = StartupApp::new(model);
        for _ in 0..10 {
            press(&mut app, KeyCode::PageDown);
        }
        assert_eq!(app.selected, 60);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        terminal
            .draw(|frame| render_startup(frame, &mut app))
            .expect("render");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("budget.59  =  12"));
        assert!(rendered.contains("Ctrl+S save"));
    }

    fn mod_package(id: &str, status: ModPackageStatus) -> ModPackageView {
        ModPackageView {
            mod_id: id.parse().expect("Mod ID"),
            version: "1.0.0".parse().expect("version"),
            status,
            dependency_count: 0,
            content: PackageContentView {
                characters: 1,
                narrator_prompts: 1,
                patches: 1,
                ..PackageContentView::default()
            },
        }
    }

    #[test]
    fn fixed_parser_accepts_six_decimal_places_without_float_rounding() {
        assert_eq!(
            parse_fixed("-12.345678"),
            Some(Fixed::from_micros(-12_345_678))
        );
        assert_eq!(
            parse_fixed("2"),
            Some(Fixed::from_integer(2).expect("fixed"))
        );
        assert_eq!(parse_fixed("1.0000001"), None);
        assert_eq!(parse_fixed("NaN"), None);
    }

    #[test]
    fn launcher_defaults_to_new_game_when_no_compatible_save_exists() {
        let mut app = StartupApp::new(fixed_model());
        assert_eq!(app.selected, 1);

        let action =
            handle_startup_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            action,
            Some(StartupAction::NewGame(StartupPlayerSelection::Fixed))
        );
    }

    #[test]
    fn launcher_mods_toggle_apply_and_cancel_use_visual_section_order() {
        let mut model = fixed_model();
        model.open_mods = true;
        model.packages.mods = vec![
            mod_package("games.loreloom.alpha", ModPackageStatus::Enabled),
            mod_package("games.loreloom.beta", ModPackageStatus::Installed),
        ];
        let mut app = StartupApp::new(model);

        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        handle_startup_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected, 1);
        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );

        let action =
            handle_startup_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let Some(StartupAction::ApplyMods { enabled }) = action else {
            panic!("Enter must apply the current Mod selection");
        };
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].mod_id.as_str(), "games.loreloom.beta");

        let mut cancel_model = fixed_model();
        cancel_model.open_mods = true;
        cancel_model.packages.mods = vec![mod_package(
            "games.loreloom.alpha",
            ModPackageStatus::Enabled,
        )];
        let mut cancel_app = StartupApp::new(cancel_model);
        handle_startup_key(
            &mut cancel_app,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        assert_eq!(
            handle_startup_key(
                &mut cancel_app,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            ),
            None
        );
        assert_eq!(cancel_app.page, StartupPage::Main);
        assert_eq!(
            cancel_app.model.packages.mods[0].status,
            ModPackageStatus::Enabled
        );
    }

    #[test]
    fn launcher_mods_render_matches_runtime_catalog_details() {
        let mut model = fixed_model();
        model.open_mods = true;
        model.packages.mods = vec![
            mod_package("games.loreloom.alpha", ModPackageStatus::Enabled),
            mod_package("games.loreloom.beta", ModPackageStatus::Installed),
        ];
        let backend = TestBackend::new(90, 32);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = StartupApp::new(model);

        terminal
            .draw(|frame| render_startup(frame, &mut app))
            .expect("render Mods");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("WORLD"));
        assert!(rendered.contains("ENABLED (1)"));
        assert!(rendered.contains("INSTALLED, NOT ENABLED (1)"));
        assert!(rendered.contains("1 definition · 1 prompt · 1 patch"));
        assert!(rendered.contains("Space toggle  Enter apply  Esc cancel"));
    }

    #[test]
    fn direct_preset_creation_starts_on_the_first_character() {
        let character = StartupPresetView {
            character_id: id("character", "one"),
            display_name: "One".to_owned(),
            summary: "First character".to_owned(),
            details: Vec::new(),
        };
        let mut model = fixed_model();
        model.new_game_only = true;
        model.player_creation = StartupPlayerCreationView::Preset {
            characters: vec![
                character.clone(),
                StartupPresetView {
                    character_id: id("character", "two"),
                    display_name: "Two".to_owned(),
                    summary: "Second character".to_owned(),
                    details: Vec::new(),
                },
            ],
        };

        let mut app = StartupApp::new(model);
        let action =
            handle_startup_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.page, StartupPage::Presets);
        assert_eq!(app.selected, 0);
        assert_eq!(
            action,
            Some(StartupAction::NewGame(StartupPlayerSelection::Preset {
                character_id: character.character_id,
            }))
        );
    }

    #[test]
    fn ugc_submission_preserves_all_seven_typed_field_kinds() {
        let choice = StartupChoiceView {
            value: id("player_option", "one"),
            display_name: "One".to_owned(),
            description: None,
        };
        let fields = vec![
            StartupFieldView {
                field_id: id("player_field", "text"),
                display_name: "Text".to_owned(),
                description: None,
                required: true,
                kind: StartupFieldKind::Text {
                    minimum_bytes: 1,
                    maximum_bytes: 32,
                    default: Some("Lin".to_owned()),
                },
            },
            StartupFieldView {
                field_id: id("player_field", "long"),
                display_name: "Long".to_owned(),
                description: None,
                required: true,
                kind: StartupFieldKind::LongText {
                    minimum_bytes: 1,
                    maximum_bytes: 64,
                    default: Some("A traveler.".to_owned()),
                },
            },
            StartupFieldView {
                field_id: id("player_field", "integer"),
                display_name: "Integer".to_owned(),
                description: None,
                required: true,
                kind: StartupFieldKind::Integer {
                    minimum: 0,
                    maximum: 10,
                    default: Some(3),
                },
            },
            StartupFieldView {
                field_id: id("player_field", "number"),
                display_name: "Number".to_owned(),
                description: None,
                required: true,
                kind: StartupFieldKind::Number {
                    minimum: Fixed::ZERO,
                    maximum: Fixed::from_integer(10).expect("fixed"),
                    default: Some(Fixed::from_micros(1_500_000)),
                },
            },
            StartupFieldView {
                field_id: id("player_field", "boolean"),
                display_name: "Boolean".to_owned(),
                description: None,
                required: true,
                kind: StartupFieldKind::Boolean { default: true },
            },
            StartupFieldView {
                field_id: id("player_field", "single"),
                display_name: "Single".to_owned(),
                description: None,
                required: true,
                kind: StartupFieldKind::SingleChoice {
                    options: vec![choice.clone()],
                    default: Some(choice.value.clone()),
                },
            },
            StartupFieldView {
                field_id: id("player_field", "multi"),
                display_name: "Multi".to_owned(),
                description: None,
                required: true,
                kind: StartupFieldKind::MultiChoice {
                    minimum_selections: 1,
                    maximum_selections: 1,
                    options: vec![choice.clone()],
                    default: BTreeSet::from([choice.value]),
                },
            },
        ];
        let form = StartupFormView {
            form_id: id("player_creation_form", "traveler"),
            display_name: "Traveler".to_owned(),
            description: "Create a traveler.".to_owned(),
            fields,
        };
        let mut state = StartupFormState::new(&form);
        assert_eq!(state.values[3], FormValueState::Number("1.5".to_owned()));

        let submission = submit_form(&form, &mut state).expect("valid typed form");

        assert_eq!(submission.values.len(), 7);
        assert!(matches!(submission.values[0].1, StartupFieldValue::Text(_)));
        assert!(matches!(submission.values[1].1, StartupFieldValue::Text(_)));
        assert!(matches!(
            submission.values[2].1,
            StartupFieldValue::Integer(3)
        ));
        assert!(matches!(
            submission.values[3].1,
            StartupFieldValue::Number(value) if value == Fixed::from_micros(1_500_000)
        ));
        assert!(matches!(
            submission.values[4].1,
            StartupFieldValue::Boolean(true)
        ));
        assert!(matches!(
            submission.values[5].1,
            StartupFieldValue::SingleChoice(_)
        ));
        assert!(matches!(
            submission.values[6].1,
            StartupFieldValue::MultiChoice(_)
        ));

        let mut invalid = StartupFormState::new(&form);
        invalid.values[0] = FormValueState::Text(String::new());
        invalid.current = form.fields.len() - 1;
        let error = submit_form(&form, &mut invalid).expect_err("name is required");
        assert_eq!(error.field_index, 0);
        assert!(error.notice.contains("Text"));

        let mut model = fixed_model();
        model.new_game_only = true;
        model.player_creation = StartupPlayerCreationView::Ugc { form };
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = StartupApp::new(model);
        terminal
            .draw(|frame| render_startup(frame, &mut app))
            .expect("render form");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("CHARACTER CARD"));
        assert!(rendered.contains("Lin"));
    }

    #[test]
    fn ugc_form_can_return_to_previous_fields_with_up_or_backtab() {
        let fields = ["name", "background"]
            .into_iter()
            .map(|key| StartupFieldView {
                field_id: id("player_field", key),
                display_name: key.to_owned(),
                description: None,
                required: false,
                kind: StartupFieldKind::Text {
                    minimum_bytes: 0,
                    maximum_bytes: 32,
                    default: None,
                },
            })
            .collect();
        let mut model = fixed_model();
        model.new_game_only = true;
        model.player_creation = StartupPlayerCreationView::Ugc {
            form: StartupFormView {
                form_id: id("player_creation_form", "traveler"),
                display_name: "Traveler".to_owned(),
                description: "Create a traveler.".to_owned(),
                fields,
            },
        };
        let mut app = StartupApp::new(model);

        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE),
        );
        handle_startup_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.form.as_ref().expect("form").current, 1);

        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE),
        );
        handle_startup_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        let form = app.form.as_ref().expect("form");
        assert_eq!(form.current, 0);
        assert_eq!(form.editor.text(), "A");

        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('C'), KeyModifiers::NONE),
        );
        handle_startup_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        );
        let form = app.form.as_ref().expect("form");
        assert_eq!(form.current, 0);
        assert_eq!(form.editor.text(), "AC");
    }

    #[test]
    fn ugc_number_validation_is_live_and_uses_compact_values() {
        let mut model = fixed_model();
        model.new_game_only = true;
        model.player_creation = StartupPlayerCreationView::Ugc {
            form: StartupFormView {
                form_id: id("player_creation_form", "traveler"),
                display_name: "Traveler".to_owned(),
                description: "Create a traveler.".to_owned(),
                fields: vec![StartupFieldView {
                    field_id: id("player_field", "resolve"),
                    display_name: "Resolve".to_owned(),
                    description: None,
                    required: true,
                    kind: StartupFieldKind::Number {
                        minimum: Fixed::ZERO,
                        maximum: Fixed::from_integer(20).expect("fixed"),
                        default: Some(Fixed::from_integer(10).expect("fixed")),
                    },
                }],
            },
        };
        let mut app = StartupApp::new(model);
        assert_eq!(app.form.as_ref().expect("form").editor.text(), "10");

        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|notice| notice.contains("number from 0 to 20"))
        );

        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        );
        assert_eq!(app.notice, None);
        handle_startup_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        );
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|notice| notice.contains("number from 0 to 20"))
        );
        assert_eq!(
            handle_startup_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),),
            None
        );
        assert_eq!(app.form.as_ref().expect("form").current, 0);
    }

    #[test]
    fn startup_recovery_keeps_settings_cancel_inside_the_app_and_allows_retry() {
        let mut model = settings_model();
        model.open_settings = false;
        model.new_game_only = true;
        model.recovery_error =
            Some("narrator: credential_environment_missing DEEPSEEK_API_KEY".into());
        let mut app = StartupApp::new(model);
        let press = |app: &mut StartupApp, code| {
            handle_startup_key(app, KeyEvent::new(code, KeyModifiers::NONE))
        };
        assert_eq!(app.page, StartupPage::Recovery);
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(StartupAction::RetryStartup)
        );
        press(&mut app, KeyCode::Down);
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(app.page, StartupPage::Settings);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('x'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert_eq!(app.page, StartupPage::Recovery);
        assert_eq!(app.model.setting_fields[0].value, "original");
        assert_eq!(
            press(&mut app, KeyCode::Esc),
            Some(StartupAction::BackToLauncher)
        );
        app.selected = 3;
        assert_eq!(press(&mut app, KeyCode::Enter), Some(StartupAction::Quit));
    }

    #[test]
    fn recovery_render_shows_diagnostic_and_repair_actions() {
        let mut model = fixed_model();
        model.recovery_error =
            Some("narrator: credential_environment_missing DEEPSEEK_API_KEY".into());
        let mut app = StartupApp::new(model);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        terminal
            .draw(|frame| render_startup(frame, &mut app))
            .expect("render");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        for expected in [
            "Unable to start",
            "credential_environment_missing",
            "DEEPSEEK_API_KEY",
            "Retry",
            "Settings",
            "Back to launcher",
            "Quit",
        ] {
            assert!(rendered.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn saves_require_explicit_confirmation_and_keep_navigation_modal() {
        let mut model = fixed_model();
        model.open_saves = true;
        model.saves = vec![StartupSaveView {
            display_name: "My save".into(),
            detail: "Most recent".into(),
        }];
        let mut app = StartupApp::new(model);
        let press = |app: &mut StartupApp, code| {
            handle_startup_key(app, KeyEvent::new(code, KeyModifiers::NONE))
        };
        assert_eq!(press(&mut app, KeyCode::Delete), None);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        terminal
            .draw(|frame| render_startup(frame, &mut app))
            .expect("render");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Delete My save?"));
        assert!(rendered.contains("[ Cancel ]"));
        assert_eq!(press(&mut app, KeyCode::Down), None);
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(StartupAction::OpenSave { index: 0 })
        );
        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.delete_confirmation, None);
        press(&mut app, KeyCode::Delete);
        press(&mut app, KeyCode::Tab);
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(StartupAction::DeleteSave { index: 0 })
        );
    }

    #[test]
    fn empty_saves_page_is_accessible_and_has_no_destructive_action() {
        let mut app = StartupApp::new(fixed_model());
        app.selected = 2;
        handle_startup_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.page, StartupPage::Saves);
        for code in [KeyCode::Enter, KeyCode::Delete] {
            assert_eq!(
                handle_startup_key(&mut app, KeyEvent::new(code, KeyModifiers::NONE)),
                None
            );
        }
        assert_eq!(app.delete_confirmation, None);
    }

    #[test]
    fn launcher_render_is_deterministic_and_contains_primary_entries() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = StartupApp::new(fixed_model());
        terminal
            .draw(|frame| render_startup(frame, &mut app))
            .expect("render");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("LORELOOM"));
        assert!(rendered.contains("New Game"));
        assert!(rendered.contains("Saves"));
        assert!(rendered.contains("Mods"));
        assert!(rendered.contains("Settings"));
    }
}
