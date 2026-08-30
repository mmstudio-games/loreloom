use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_INPUT_BYTES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EditorError {
    #[error("input exceeds the configured byte limit")]
    TooLong,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputEditor {
    text: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    history_draft: Option<String>,
}

impl InputEditor {
    pub fn with_text(text: impl Into<String>) -> Result<Self, EditorError> {
        let text = text.into();
        if text.len() > MAX_INPUT_BYTES {
            return Err(EditorError::TooLong);
        }
        let cursor = text.graphemes(true).count();
        Ok(Self {
            text,
            cursor,
            history: Vec::new(),
            history_index: None,
            history_draft: None,
        })
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    #[must_use]
    pub fn history(&self) -> &[String] {
        &self.history
    }

    #[must_use]
    pub const fn history_index(&self) -> Option<usize> {
        self.history_index
    }

    #[must_use]
    pub fn grapheme_count(&self) -> usize {
        self.text.graphemes(true).count()
    }

    pub fn insert(&mut self, value: &str) -> Result<(), EditorError> {
        if self.text.len().saturating_add(value.len()) > MAX_INPUT_BYTES {
            return Err(EditorError::TooLong);
        }
        self.detach_history();
        let byte_index = self.byte_index(self.cursor);
        self.text.insert_str(byte_index, value);
        let insertion_end = byte_index + value.len();
        self.cursor = self.grapheme_cursor_at_or_after(insertion_end);
        Ok(())
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.cursor = self.cursor.saturating_add(1).min(self.grapheme_count());
    }

    pub fn move_home(&mut self) {
        let graphemes = self.graphemes();
        self.cursor = line_start(&graphemes, self.cursor);
    }

    pub fn move_end(&mut self) {
        let graphemes = self.graphemes();
        self.cursor = line_end(&graphemes, self.cursor);
    }

    pub fn move_up(&mut self) {
        let graphemes = self.graphemes();
        let start = line_start(&graphemes, self.cursor);
        if start == 0 {
            return;
        }
        let column = self.cursor.saturating_sub(start);
        let previous_end = start - 1;
        let previous_start = line_start(&graphemes, previous_end);
        self.cursor = previous_start + column.min(previous_end - previous_start);
    }

    pub fn move_down(&mut self) {
        let graphemes = self.graphemes();
        let end = line_end(&graphemes, self.cursor);
        if end == graphemes.len() {
            return;
        }
        let start = line_start(&graphemes, self.cursor);
        let column = self.cursor.saturating_sub(start);
        let next_start = end + 1;
        let next_end = line_end(&graphemes, next_start);
        self.cursor = next_start + column.min(next_end - next_start);
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.detach_history();
        let start = self.byte_index(self.cursor - 1);
        let end = self.byte_index(self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        if self.cursor == self.grapheme_count() {
            return;
        }
        self.detach_history();
        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    pub fn history_previous(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = match self.history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.history_draft = Some(self.text.clone());
                self.history.len() - 1
            }
        };
        self.select_history(index);
    }

    pub fn history_next(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 < self.history.len() {
            self.select_history(index + 1);
        } else {
            self.text = self.history_draft.take().unwrap_or_default();
            self.cursor = self.grapheme_count();
            self.history_index = None;
        }
    }

    pub fn submit(&mut self) -> Option<String> {
        if self.text.trim().is_empty() {
            return None;
        }
        let submitted = std::mem::take(&mut self.text);
        self.history.push(submitted.clone());
        self.cursor = 0;
        self.history_index = None;
        self.history_draft = None;
        Some(submitted)
    }

    pub fn restore_failed_submission(&mut self, submitted: String) {
        if self.history.last() == Some(&submitted) {
            self.history.pop();
        }
        self.text = submitted;
        self.cursor = self.grapheme_count();
        self.history_index = None;
        self.history_draft = None;
    }

    #[must_use]
    pub fn text_with_cursor(&self) -> String {
        let mut rendered = self.text.clone();
        rendered.insert(self.byte_index(self.cursor), '▏');
        rendered
    }

    fn byte_index(&self, grapheme_index: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .nth(grapheme_index)
            .map_or(self.text.len(), |(byte_index, _)| byte_index)
    }

    fn grapheme_cursor_at_or_after(&self, byte_index: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .enumerate()
            .find_map(|(cursor, (start, _))| (start >= byte_index).then_some(cursor))
            .unwrap_or_else(|| self.grapheme_count())
    }

    fn graphemes(&self) -> Vec<&str> {
        self.text.graphemes(true).collect()
    }

    fn select_history(&mut self, index: usize) {
        self.text = self.history[index].clone();
        self.cursor = self.grapheme_count();
        self.history_index = Some(index);
    }

    fn detach_history(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }
}

fn line_start(graphemes: &[&str], cursor: usize) -> usize {
    graphemes[..cursor.min(graphemes.len())]
        .iter()
        .rposition(|grapheme| is_newline(grapheme))
        .map_or(0, |index| index + 1)
}

fn line_end(graphemes: &[&str], cursor: usize) -> usize {
    graphemes[cursor.min(graphemes.len())..]
        .iter()
        .position(|grapheme| is_newline(grapheme))
        .map_or(graphemes.len(), |offset| cursor + offset)
}

fn is_newline(grapheme: &str) -> bool {
    matches!(grapheme, "\n" | "\r\n")
}
