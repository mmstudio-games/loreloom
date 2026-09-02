//! Loreloom terminal input, rendering, session lifecycle, and Runtime client boundary.

mod editor;
mod render;
mod run;
mod session;
mod startup;
mod state;

pub use editor::{EditorError, InputEditor, MAX_INPUT_BYTES};
pub use render::{WIDE_LAYOUT_MINIMUM, render_ui};
pub use run::{RuntimeClient, TuiConfig, TuiError, run};
pub use session::{CrosstermTerminalOps, TerminalOps, TerminalSession};
pub use startup::{
    StartupAction, StartupApp, StartupChoiceView, StartupFieldKind, StartupFieldValue,
    StartupFieldView, StartupFormSubmission, StartupFormView, StartupModel, StartupPage,
    StartupPlayerCreationView, StartupPlayerSelection, StartupPresetView, StartupSaveView,
    handle_startup_key, render_startup, run_startup,
};
pub use state::{
    NarrowPage, RuntimeUiEvent, TuiApp, TuiOverlay, UiClientError, UiIntent, handle_key,
    handle_mouse, handle_paste,
};
