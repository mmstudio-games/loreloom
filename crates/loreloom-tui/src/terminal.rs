use std::io;

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{CrosstermTerminalOps, TerminalSession, TuiError};

/// One interactive alternate-screen lifetime shared by Loreloom's startup and game pages.
pub struct TuiTerminal {
    pub(crate) terminal: Terminal<CrosstermBackend<io::Stdout>>,
    pub(crate) session: TerminalSession<CrosstermTerminalOps>,
}

impl TuiTerminal {
    /// Opens raw mode and the alternate screen. Dropping the value restores the terminal.
    pub fn open() -> Result<Self, TuiError> {
        let session = TerminalSession::open(CrosstermTerminalOps)?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal, session })
    }

    pub(crate) fn show_loading(&mut self, world_name: &str) -> Result<(), TuiError> {
        self.terminal
            .draw(|frame| render_loading(frame, world_name))?;
        Ok(())
    }
}

fn render_loading(frame: &mut ratatui::Frame<'_>, world_name: &str) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(4),
            Constraint::Fill(1),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                world_name.to_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Preparing the world…",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .alignment(Alignment::Center),
        rows[1],
    );
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, widgets::Paragraph};

    use super::*;

    #[test]
    fn loading_page_replaces_previous_terminal_contents() {
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("OLD STARTUP PAGE"), frame.area()))
            .expect("render previous page");
        terminal
            .draw(|frame| render_loading(frame, "Rainbound Inn"))
            .expect("render loading page");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("Rainbound Inn"));
        assert!(rendered.contains("Preparing the world…"));
        assert!(!rendered.contains("OLD STARTUP PAGE"));
    }
}
