use std::{io, time::Duration};

use crossterm::event::{self, Event};
use loreloom_core::{RuntimePhase, UiSnapshot};
use ratatui::{Terminal, backend::CrosstermBackend};
use thiserror::Error;

use crate::{
    CrosstermTerminalOps, RuntimeUiEvent, TerminalSession, TuiApp, UiClientError, UiIntent,
    handle_key, handle_mouse, handle_paste, render::render_ui_with_state_width,
};

const MAX_RUNTIME_EVENTS_PER_FRAME: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiConfig {
    pub state_width_percent: u16,
    pub event_poll_interval: Duration,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            state_width_percent: 30,
            event_poll_interval: Duration::from_millis(50),
        }
    }
}

impl TuiConfig {
    fn validate(self) -> Result<Self, TuiError> {
        if !(25..=35).contains(&self.state_width_percent) || self.event_poll_interval.is_zero() {
            return Err(TuiError::InvalidConfig);
        }
        Ok(self)
    }
}

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("TUI configuration is invalid")]
    InvalidConfig,
    #[error(transparent)]
    Terminal(#[from] io::Error),
    #[error(transparent)]
    Client(#[from] UiClientError),
}

pub trait RuntimeClient {
    fn submit(&mut self, input: String) -> Result<(), UiClientError>;
    fn cancel(&mut self) -> Result<(), UiClientError>;
    fn try_recv(&mut self) -> Result<Option<RuntimeUiEvent>, UiClientError>;
    fn shutdown(&mut self) -> Result<(), UiClientError>;
}

pub fn run(
    client: &mut impl RuntimeClient,
    initial_snapshot: UiSnapshot,
    config: TuiConfig,
) -> Result<(), TuiError> {
    let config = config.validate()?;
    let _session = TerminalSession::open(CrosstermTerminalOps)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    let mut app = TuiApp::new(initial_snapshot);

    let loop_result = run_loop(client, &mut terminal, &mut app, config);
    let shutdown_result = client.shutdown().map_err(TuiError::Client);
    loop_result.and(shutdown_result)
}

fn run_loop(
    client: &mut impl RuntimeClient,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    config: TuiConfig,
) -> Result<(), TuiError> {
    loop {
        for _ in 0..MAX_RUNTIME_EVENTS_PER_FRAME {
            let Some(event) = client.try_recv()? else {
                break;
            };
            app.apply_runtime_event(event);
        }
        app.tick_spinner();
        terminal.draw(|frame| {
            render_ui_with_state_width(frame, app, config.state_width_percent);
        })?;

        if !event::poll(config.event_poll_interval)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => match handle_key(app, key) {
                Some(UiIntent::Submit(input)) => {
                    if let Err(error) = client.submit(input.clone()) {
                        app.editor.restore_failed_submission(input);
                        return Err(error.into());
                    }
                    app.show_submitted_input(input);
                    app.apply_runtime_event(RuntimeUiEvent::PhaseChanged(
                        RuntimePhase::PersistingInput,
                    ));
                }
                Some(UiIntent::Cancel) => client.cancel()?,
                Some(UiIntent::Quit) => return Ok(()),
                None => {}
            },
            Event::Paste(text) => {
                let _ = handle_paste(app, &text);
            }
            Event::Mouse(mouse) => handle_mouse(app, mouse),
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
        }
    }
}
