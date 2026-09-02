use std::{collections::BTreeSet, io};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use loreloom_core::{ContentDefinitionId, Fixed, ModPackageStatus, PackageCatalogView};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::{CrosstermTerminalOps, InputEditor, TerminalSession, TuiConfig, TuiError};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupModel {
    pub world_name: String,
    pub world_id: String,
    pub saves: Vec<StartupSaveView>,
    pub packages: PackageCatalogView,
    pub settings: Vec<String>,
    pub player_creation: StartupPlayerCreationView,
    pub new_game_only: bool,
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
    OpenSave { index: usize },
    NewGame(StartupPlayerSelection),
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
                StartupFieldKind::Number { default, .. } => FormValueState::Number(
                    default.map_or_else(String::new, |value| value.to_string()),
                ),
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
    pub model: StartupModel,
    pub page: StartupPage,
    pub selected: usize,
    pub notice: Option<String>,
    form: Option<StartupFormState>,
}

impl StartupApp {
    #[must_use]
    pub fn new(model: StartupModel) -> Self {
        let page = if model.new_game_only {
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
        } else {
            0
        };
        Self {
            model,
            page,
            selected,
            notice: None,
            form,
        }
    }

    fn return_to_main(&mut self) -> Option<StartupAction> {
        if self.model.new_game_only {
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
    if model.new_game_only && matches!(model.player_creation, StartupPlayerCreationView::Fixed) {
        return Ok(StartupAction::NewGame(StartupPlayerSelection::Fixed));
    }
    let config = config.validate()?;
    let _session = TerminalSession::open(CrosstermTerminalOps)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    let mut app = StartupApp::new(model);
    loop {
        terminal.draw(|frame| render_startup(frame, &mut app))?;
        if !event::poll(config.event_poll_interval)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                if let Some(action) = handle_startup_key(&mut app, key) {
                    return Ok(action);
                }
            }
            Event::Paste(text) => handle_startup_paste(&mut app, &text),
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
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
    match app.page {
        StartupPage::Main => handle_main_key(app, key),
        StartupPage::Saves => handle_saves_key(app, key),
        StartupPage::Mods | StartupPage::Settings => handle_information_key(app, key),
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
            2 if !app.model.saves.is_empty() => {
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
    !matches!(index, 0 | 2) || !app.model.saves.is_empty()
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

fn handle_information_key(app: &mut StartupApp, key: KeyEvent) -> Option<StartupAction> {
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => return app.return_to_main(),
        KeyCode::Up | KeyCode::PageUp => app.selected = app.selected.saturating_sub(1),
        KeyCode::Down | KeyCode::PageDown => app.selected = app.selected.saturating_add(1),
        _ => {}
    }
    None
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
    match key.code {
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            state.select(current.saturating_sub(1));
            app.notice = None;
        }
        KeyCode::Tab => {
            state.select((current + 1).min(form.fields.len() - 1));
            app.notice = None;
        }
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::ALT)
                && matches!(field.kind, StartupFieldKind::LongText { .. }) =>
        {
            insert_form_text(state, field, "\n");
        }
        KeyCode::Char('j')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(field.kind, StartupFieldKind::LongText { .. }) =>
        {
            insert_form_text(state, field, "\n");
        }
        KeyCode::Enter => {
            state.store_editor();
            if current + 1 < form.fields.len() {
                state.select(current + 1);
                app.notice = None;
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
        KeyCode::Up => adjust_form_value(state, field, -1),
        KeyCode::Down => adjust_form_value(state, field, 1),
        KeyCode::Left => {
            if editor_text(&state.values[current]).is_some() {
                state.editor.move_left();
            } else {
                adjust_form_value(state, field, -1);
            }
        }
        KeyCode::Right => {
            if editor_text(&state.values[current]).is_some() {
                state.editor.move_right();
            } else {
                adjust_form_value(state, field, 1);
            }
        }
        KeyCode::Char(' ') => toggle_form_value(state, field),
        KeyCode::Home => state.editor.move_home(),
        KeyCode::End => state.editor.move_end(),
        KeyCode::Backspace => state.editor.backspace(),
        KeyCode::Delete => state.editor.delete(),
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            insert_form_text(state, field, &character.to_string());
        }
        _ => {}
    }
    None
}

fn handle_startup_paste(app: &mut StartupApp, text: &str) {
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
            *raw = adjusted.to_string();
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
        let invalid = || FormValidationError {
            field_index,
            notice: format!("{} · invalid_value", field.field_id),
        };
        let value = match (&field.kind, value) {
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
            ) if value.len() >= *minimum_bytes as usize
                && value.len() <= *maximum_bytes as usize =>
            {
                StartupFieldValue::Text(value.clone())
            }
            (
                StartupFieldKind::Integer {
                    minimum, maximum, ..
                },
                FormValueState::Integer(raw),
            ) => {
                let value = raw.parse::<i64>().map_err(|_| invalid())?;
                if value < *minimum || value > *maximum {
                    return Err(invalid());
                }
                StartupFieldValue::Integer(value)
            }
            (
                StartupFieldKind::Number {
                    minimum, maximum, ..
                },
                FormValueState::Number(raw),
            ) => {
                let value = parse_fixed(raw).ok_or_else(invalid)?;
                if value < *minimum || value > *maximum {
                    return Err(invalid());
                }
                StartupFieldValue::Number(value)
            }
            (StartupFieldKind::Boolean { .. }, FormValueState::Boolean(value)) => {
                StartupFieldValue::Boolean(*value)
            }
            (
                StartupFieldKind::SingleChoice { options, .. },
                FormValueState::SingleChoice(Some(value)),
            ) if options.iter().any(|option| &option.value == value) => {
                StartupFieldValue::SingleChoice(value.clone())
            }
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
                StartupFieldValue::MultiChoice(selected.clone())
            }
            (_, FormValueState::Text(value))
            | (_, FormValueState::Integer(value))
            | (_, FormValueState::Number(value))
                if !field.required && value.is_empty() =>
            {
                continue;
            }
            (_, FormValueState::SingleChoice(None)) if !field.required => continue,
            _ => return Err(invalid()),
        };
        values.push((field.field_id.clone(), value));
    }
    Ok(StartupFormSubmission {
        form_id: form.form_id.clone(),
        values,
    })
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
        StartupPage::Main => render_main(frame, app, rows[1]),
        StartupPage::Saves => render_saves(frame, app, rows[1]),
        StartupPage::Mods => render_mods(frame, app, rows[1]),
        StartupPage::Settings => render_settings(frame, app, rows[1]),
        StartupPage::Presets => render_presets(frame, app, rows[1]),
        StartupPage::Form => render_form(frame, app, rows[1]),
    }
    render_startup_footer(frame, app, rows[2]);
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
        ("Load Save", !app.model.saves.is_empty()),
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
        "LOAD SAVE",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    ))];
    for (index, save) in app.model.saves.iter().enumerate() {
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

fn render_mods(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let mut lines = vec![Line::from(Span::styled(
        "MODS",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(format!(
        "World  {}  v{}",
        app.model.packages.world.world_id, app.model.packages.world.version
    )));
    for package in &app.model.packages.mods {
        lines.push(Line::from(format!(
            "{}  v{}  {}  {} definitions",
            package.mod_id,
            package.version,
            mod_status_label(package.status),
            package.content.definition_count()
        )));
    }
    if app.model.packages.unavailable_installed > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "{} installed package(s) unavailable",
                app.model.packages.unavailable_installed
            ),
            Style::default().fg(Color::Yellow),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((u16::try_from(app.selected).unwrap_or(u16::MAX), 0))
            .wrap(Wrap { trim: false }),
        inset(area, 2, 1),
    );
}

fn mod_status_label(status: ModPackageStatus) -> &'static str {
    match status {
        ModPackageStatus::Enabled => "enabled",
        ModPackageStatus::Installed => "installed",
    }
}

fn render_settings(frame: &mut Frame<'_>, app: &StartupApp, area: Rect) {
    let mut lines = vec![Line::from(Span::styled(
        "SETTINGS",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    ))];
    lines.extend(app.model.settings.iter().cloned().map(Line::from));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inset(area, 2, 1),
    );
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
        fields.push(Line::from(vec![
            Span::styled(
                if selected { "› " } else { "  " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                format!(
                    "{}{}",
                    field.display_name,
                    if field.required { " *" } else { "" }
                ),
                if selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
            Span::styled(
                format!("  {}", display_form_value(field, value, state, index)),
                Style::default().fg(if selected { ACCENT } else { MUTED }),
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
        StartupPage::Main => "↑↓ select  Enter open  Esc quit",
        StartupPage::Saves | StartupPage::Presets => "↑↓ select  Enter confirm  Esc back",
        StartupPage::Mods | StartupPage::Settings => "↑↓ scroll  Esc back",
        StartupPage::Form => "Tab field  ↑↓/Space choose  Enter next/confirm  Esc back",
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
    use loreloom_core::{PackageCatalogView, WorldPackageView};
    use ratatui::backend::TestBackend;

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
            player_creation: StartupPlayerCreationView::Fixed,
            new_game_only: false,
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
        assert!(error.notice.contains("player_field/text"));

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
        assert!(rendered.contains("Load Save"));
        assert!(rendered.contains("Mods"));
        assert!(rendered.contains("Settings"));
    }
}
