use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use loreloom_core::{RuntimePhase, UiSnapshot};
use thiserror::Error;

use crate::{EditorError, InputEditor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrowPage {
    State,
    Story,
}

impl NarrowPage {
    pub fn toggle(&mut self) {
        *self = match self {
            Self::State => Self::Story,
            Self::Story => Self::State,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeUiEvent {
    Snapshot(Box<UiSnapshot>),
    PhaseChanged(RuntimePhase),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("runtime client failed: {code}")]
pub struct UiClientError {
    pub code: &'static str,
}

impl UiClientError {
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self { code }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiIntent {
    Submit(String),
    Cancel,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiApp {
    pub snapshot: UiSnapshot,
    pub editor: InputEditor,
    pub narrow_page: NarrowPage,
    pub transcript_scroll: u16,
    pub working_phase: Option<RuntimePhase>,
    pub spinner_frame: u8,
}

impl TuiApp {
    #[must_use]
    pub fn new(snapshot: UiSnapshot) -> Self {
        Self {
            snapshot,
            editor: InputEditor::default(),
            narrow_page: NarrowPage::Story,
            transcript_scroll: 0,
            working_phase: None,
            spinner_frame: 0,
        }
    }

    pub fn apply_runtime_event(&mut self, event: RuntimeUiEvent) {
        match event {
            RuntimeUiEvent::Snapshot(snapshot) => {
                if matches!(
                    snapshot.phase,
                    RuntimePhase::Idle
                        | RuntimePhase::Completed
                        | RuntimePhase::Cancelled
                        | RuntimePhase::Failed
                ) {
                    self.working_phase = None;
                }
                self.snapshot = *snapshot;
            }
            RuntimeUiEvent::PhaseChanged(phase) => {
                if matches!(
                    phase,
                    RuntimePhase::Idle
                        | RuntimePhase::Completed
                        | RuntimePhase::Cancelled
                        | RuntimePhase::Failed
                ) {
                    self.working_phase = None;
                } else {
                    self.working_phase = Some(phase);
                    self.spinner_frame = 0;
                }
            }
        }
    }

    pub fn tick_spinner(&mut self) {
        if self.working_phase.is_some() {
            self.spinner_frame = self.spinner_frame.wrapping_add(1) % 10;
        }
    }

    #[must_use]
    pub fn effective_phase(&self) -> RuntimePhase {
        self.working_phase.unwrap_or(self.snapshot.phase)
    }

    #[must_use]
    pub fn can_submit(&self) -> bool {
        self.working_phase.is_none() && self.snapshot.can_submit
    }

    #[must_use]
    pub fn can_cancel(&self) -> bool {
        self.working_phase.is_some() || self.snapshot.can_cancel
    }

    pub fn scroll_up(&mut self) {
        self.transcript_scroll = self.transcript_scroll.saturating_add(1);
    }

    pub fn scroll_down(&mut self) {
        self.transcript_scroll = self.transcript_scroll.saturating_sub(1);
    }
}

pub fn handle_key(app: &mut TuiApp, key: KeyEvent) -> Option<UiIntent> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(UiIntent::Quit)
        }
        (KeyCode::Enter, modifiers) if modifiers.contains(KeyModifiers::ALT) => {
            let _ = app.editor.insert("\n");
            None
        }
        (KeyCode::Char('j'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            let _ = app.editor.insert("\n");
            None
        }
        (KeyCode::Enter, KeyModifiers::NONE) if app.can_submit() => {
            app.editor.submit().map(UiIntent::Submit)
        }
        (KeyCode::Esc, _) if app.can_cancel() => Some(UiIntent::Cancel),
        (KeyCode::Tab, _) => {
            app.narrow_page.toggle();
            None
        }
        (KeyCode::PageUp, _) => {
            app.scroll_up();
            None
        }
        (KeyCode::PageDown, _) => {
            app.scroll_down();
            None
        }
        (KeyCode::Char('p'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            app.editor.history_previous();
            None
        }
        (KeyCode::Char('n'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            app.editor.history_next();
            None
        }
        (KeyCode::Left, _) => {
            app.editor.move_left();
            None
        }
        (KeyCode::Right, _) => {
            app.editor.move_right();
            None
        }
        (KeyCode::Up, _) => {
            app.editor.move_up();
            None
        }
        (KeyCode::Down, _) => {
            app.editor.move_down();
            None
        }
        (KeyCode::Home, _) => {
            app.editor.move_home();
            None
        }
        (KeyCode::End, _) => {
            app.editor.move_end();
            None
        }
        (KeyCode::Backspace, _) => {
            app.editor.backspace();
            None
        }
        (KeyCode::Delete, _) => {
            app.editor.delete();
            None
        }
        (KeyCode::Char(character), modifiers)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let _ = app.editor.insert(&character.to_string());
            None
        }
        _ => None,
    }
}

pub fn handle_paste(app: &mut TuiApp, text: &str) -> Result<(), EditorError> {
    app.editor.insert(text)
}
