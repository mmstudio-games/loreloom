use std::io;

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

pub trait TerminalOps {
    type Error;

    fn enable_raw_mode(&mut self) -> Result<(), Self::Error>;
    fn disable_raw_mode(&mut self) -> Result<(), Self::Error>;
    fn enter_alternate_screen(&mut self) -> Result<(), Self::Error>;
    fn leave_alternate_screen(&mut self) -> Result<(), Self::Error>;
    fn hide_cursor(&mut self) -> Result<(), Self::Error>;
    fn show_cursor(&mut self) -> Result<(), Self::Error>;
    fn enable_bracketed_paste(&mut self) -> Result<(), Self::Error>;
    fn disable_bracketed_paste(&mut self) -> Result<(), Self::Error>;
    fn enable_mouse_capture(&mut self) -> Result<(), Self::Error>;
    fn disable_mouse_capture(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CrosstermTerminalOps;

impl TerminalOps for CrosstermTerminalOps {
    type Error = io::Error;

    fn enable_raw_mode(&mut self) -> Result<(), Self::Error> {
        enable_raw_mode()
    }

    fn disable_raw_mode(&mut self) -> Result<(), Self::Error> {
        disable_raw_mode()
    }

    fn enter_alternate_screen(&mut self) -> Result<(), Self::Error> {
        execute!(io::stdout(), EnterAlternateScreen)
    }

    fn leave_alternate_screen(&mut self) -> Result<(), Self::Error> {
        execute!(io::stdout(), LeaveAlternateScreen)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        execute!(io::stdout(), Hide)
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        execute!(io::stdout(), Show)
    }

    fn enable_bracketed_paste(&mut self) -> Result<(), Self::Error> {
        execute!(io::stdout(), EnableBracketedPaste)
    }

    fn disable_bracketed_paste(&mut self) -> Result<(), Self::Error> {
        execute!(io::stdout(), DisableBracketedPaste)
    }

    fn enable_mouse_capture(&mut self) -> Result<(), Self::Error> {
        execute!(io::stdout(), EnableMouseCapture)
    }

    fn disable_mouse_capture(&mut self) -> Result<(), Self::Error> {
        execute!(io::stdout(), DisableMouseCapture)
    }
}

pub struct TerminalSession<T: TerminalOps> {
    ops: T,
    raw_mode: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
    bracketed_paste: bool,
    mouse_capture: bool,
}

impl<T: TerminalOps> TerminalSession<T> {
    pub fn open(ops: T) -> Result<Self, T::Error> {
        let mut session = Self {
            ops,
            raw_mode: false,
            alternate_screen: false,
            cursor_hidden: false,
            bracketed_paste: false,
            mouse_capture: false,
        };
        session.ops.enable_raw_mode()?;
        session.raw_mode = true;
        session.ops.enter_alternate_screen()?;
        session.alternate_screen = true;
        session.ops.hide_cursor()?;
        session.cursor_hidden = true;
        session.ops.enable_bracketed_paste()?;
        session.bracketed_paste = true;
        session.ops.enable_mouse_capture()?;
        session.mouse_capture = true;
        Ok(session)
    }
}

impl<T: TerminalOps> Drop for TerminalSession<T> {
    fn drop(&mut self) {
        if self.mouse_capture {
            let _ = self.ops.disable_mouse_capture();
        }
        if self.bracketed_paste {
            let _ = self.ops.disable_bracketed_paste();
        }
        if self.cursor_hidden {
            let _ = self.ops.show_cursor();
        }
        if self.alternate_screen {
            let _ = self.ops.leave_alternate_screen();
        }
        if self.raw_mode {
            let _ = self.ops.disable_raw_mode();
        }
    }
}
