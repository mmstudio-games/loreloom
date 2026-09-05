use std::{io, time::Duration};

use crossterm::event::{self, Event};
use loreloom_appearance::AppearanceCatalog;
use loreloom_core::{RuntimePhase, UiSnapshot};
use ratatui::layout::Size;
use ratatui::{Terminal, backend::CrosstermBackend};
use ratatui_image::picker::Picker;
use thiserror::Error;

use crate::{
    ImageProtocolPreference, RuntimeUiEvent, TuiApp, TuiTerminal, UiClientError, UiIntent,
    appearance::AppearancePresenter, handle_key, handle_mouse, handle_paste,
    render::render_ui_with_appearance,
};

const MAX_RUNTIME_EVENTS_PER_FRAME: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiConfig {
    pub state_width_percent: u16,
    pub event_poll_interval: Duration,
    pub image_protocol: ImageProtocolPreference,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            state_width_percent: 30,
            event_poll_interval: Duration::from_millis(50),
            image_protocol: ImageProtocolPreference::Auto,
        }
    }
}

impl TuiConfig {
    pub(crate) fn validate(self) -> Result<Self, TuiError> {
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
    run_with_appearance(
        client,
        initial_snapshot,
        config,
        AppearanceCatalog::default(),
    )
}

pub fn run_with_appearance(
    client: &mut impl RuntimeClient,
    initial_snapshot: UiSnapshot,
    config: TuiConfig,
    appearance: AppearanceCatalog,
) -> Result<(), TuiError> {
    TuiTerminal::open()?.run_with_appearance(client, initial_snapshot, config, appearance)
}

impl TuiTerminal {
    pub fn run_with_appearance(
        mut self,
        client: &mut impl RuntimeClient,
        initial_snapshot: UiSnapshot,
        config: TuiConfig,
        appearance: AppearanceCatalog,
    ) -> Result<(), TuiError> {
        let config = config.validate()?;
        let mut presenter = if config.image_protocol == ImageProtocolPreference::Disabled
            || appearance.is_empty()
        {
            None
        } else {
            let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
            if let Some(protocol) = config.image_protocol.forced() {
                picker.set_protocol_type(protocol);
            }
            Some(AppearancePresenter::new(appearance, picker)?)
        };
        self.terminal.clear()?;
        let mut app = TuiApp::new(initial_snapshot);

        let loop_result = run_loop(
            client,
            &mut self.terminal,
            &mut app,
            config,
            presenter.as_mut(),
        );
        let shutdown_result = client.shutdown().map_err(TuiError::Client);
        let Self { terminal, session } = self;
        drop(terminal);
        drop(session);
        // A bounded compositor job may still be finishing. Restore the terminal before joining it.
        drop(presenter);
        loop_result.and(shutdown_result)
    }
}

fn run_loop(
    client: &mut impl RuntimeClient,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    config: TuiConfig,
    mut appearance: Option<&mut AppearancePresenter>,
) -> Result<(), TuiError> {
    loop {
        for _ in 0..MAX_RUNTIME_EVENTS_PER_FRAME {
            let Some(event) = client.try_recv()? else {
                break;
            };
            app.apply_runtime_event(event);
        }
        app.tick_spinner();
        if let Some(presenter) = appearance.as_deref_mut() {
            presenter.sync(&app.snapshot, appearance_target(terminal.size()?, config));
        }
        terminal.draw(|frame| {
            let protocol = appearance
                .as_deref()
                .and_then(AppearancePresenter::protocol);
            let status = appearance.as_deref().and_then(AppearancePresenter::status);
            render_ui_with_appearance(frame, app, config.state_width_percent, protocol, status);
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

fn appearance_target(size: Size, config: TuiConfig) -> Option<Size> {
    if size.width < crate::WIDE_LAYOUT_MINIMUM {
        return None;
    }
    let width = size
        .width
        .saturating_mul(config.state_width_percent)
        .saturating_div(100)
        .saturating_sub(2);
    let height = size.height.saturating_sub(15).min(28);
    (width >= 8 && height >= 6).then_some(Size::new(width, height))
}
