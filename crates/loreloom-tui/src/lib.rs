//! Loreloom terminal input, rendering, session lifecycle, and Runtime client boundary.

mod editor;
mod render;
mod run;
mod session;
mod state;

pub use editor::{EditorError, InputEditor, MAX_INPUT_BYTES};
pub use render::{WIDE_LAYOUT_MINIMUM, render_ui};
pub use run::{RuntimeClient, TuiConfig, TuiError, run};
pub use session::{CrosstermTerminalOps, TerminalOps, TerminalSession};
pub use state::{
    NarrowPage, RuntimeUiEvent, TuiApp, UiClientError, UiIntent, handle_key, handle_paste,
};
