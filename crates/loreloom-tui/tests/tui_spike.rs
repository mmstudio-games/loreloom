//! P0 evidence for Loreloom's terminal interaction boundary.
//!
//! Every type in this file is deliberately test-only. The spike validates the
//! proposed interaction without freezing a public TUI view model or event API.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Terminal, TerminalOptions, Viewport};
use unicode_segmentation::UnicodeSegmentation;

const WIDE_LAYOUT_MINIMUM: u16 = 80;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct InputEditor {
    text: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
}

impl InputEditor {
    fn with_text(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            cursor: text.graphemes(true).count(),
            history: Vec::new(),
            history_index: None,
        }
    }

    fn byte_index(&self, grapheme_index: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .nth(grapheme_index)
            .map_or(self.text.len(), |(byte_index, _)| byte_index)
    }

    fn grapheme_count(&self) -> usize {
        self.text.graphemes(true).count()
    }

    fn insert(&mut self, value: &str) {
        let byte_index = self.byte_index(self.cursor);
        self.text.insert_str(byte_index, value);
        self.cursor += value.graphemes(true).count();
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor = self.cursor.saturating_add(1).min(self.grapheme_count());
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_index(self.cursor - 1);
        let end = self.byte_index(self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        if self.cursor == self.grapheme_count() {
            return;
        }
        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    fn submit(&mut self) -> Option<String> {
        if self.text.trim().is_empty() {
            return None;
        }
        let submitted = std::mem::take(&mut self.text);
        self.history.push(submitted.clone());
        self.history_index = None;
        self.cursor = 0;
        Some(submitted)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NarrowPage {
    State,
    Story,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamState {
    Live,
    Final,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StreamRow {
    text: String,
    state: StreamState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolState {
    Pending,
    Succeeded,
    Rejected,
    Failed,
}

impl ToolState {
    const fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }

    const fn color(self) -> Color {
        match self {
            Self::Pending => Color::Yellow,
            Self::Succeeded => Color::Green,
            Self::Rejected => Color::Magenta,
            Self::Failed => Color::Red,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ToolRow {
    name: String,
    state: ToolState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TranscriptRow {
    speaker: String,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AppState {
    character_name: String,
    location: String,
    health: (u16, u16),
    stamina: (u16, u16),
    transcript: Vec<TranscriptRow>,
    tools: Vec<ToolRow>,
    stream: Option<StreamRow>,
    editor: InputEditor,
    running: bool,
    narrow_page: NarrowPage,
    transcript_scroll: u16,
}

impl AppState {
    fn sample() -> Self {
        Self {
            character_name: "Aster".to_owned(),
            location: "Old Mill".to_owned(),
            health: (17, 20),
            stamina: (4, 12),
            transcript: vec![
                TranscriptRow {
                    speaker: "You".to_owned(),
                    text: "Ask Mira about the bell.".to_owned(),
                },
                TranscriptRow {
                    speaker: "Mira".to_owned(),
                    text: "It rang before dawn.".to_owned(),
                },
            ],
            tools: vec![
                ToolRow {
                    name: "observe_scene".to_owned(),
                    state: ToolState::Pending,
                },
                ToolRow {
                    name: "npc_speak".to_owned(),
                    state: ToolState::Succeeded,
                },
                ToolRow {
                    name: "take_crown".to_owned(),
                    state: ToolState::Rejected,
                },
                ToolRow {
                    name: "provider_call".to_owned(),
                    state: ToolState::Failed,
                },
            ],
            stream: Some(StreamRow {
                text: "Dust turns in the amber light...".to_owned(),
                state: StreamState::Live,
            }),
            editor: InputEditor::with_text("Look closer"),
            running: true,
            narrow_page: NarrowPage::Story,
            transcript_scroll: 1,
        }
    }

    fn append_stream_chunk(&mut self, chunk: &str) {
        let stream = self.stream.get_or_insert_with(|| StreamRow {
            text: String::new(),
            state: StreamState::Live,
        });
        stream.text.push_str(chunk);
        stream.state = StreamState::Live;
    }

    fn finish_stream(&mut self, state: StreamState) {
        if let Some(stream) = &mut self.stream {
            stream.state = state;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum UiIntent {
    Submit(String),
    Cancel,
    Quit,
}

fn handle_key(state: &mut AppState, key: KeyEvent) -> Option<UiIntent> {
    match (key.code, key.modifiers) {
        (KeyCode::Enter, KeyModifiers::ALT) | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
            state.editor.insert("\n");
            None
        }
        (KeyCode::Enter, KeyModifiers::NONE) => state.editor.submit().map(UiIntent::Submit),
        (KeyCode::Esc, _) if state.running => Some(UiIntent::Cancel),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(UiIntent::Quit),
        (KeyCode::Left, _) => {
            state.editor.move_left();
            None
        }
        (KeyCode::Right, _) => {
            state.editor.move_right();
            None
        }
        (KeyCode::Backspace, _) => {
            state.editor.backspace();
            None
        }
        (KeyCode::Delete, _) => {
            state.editor.delete();
            None
        }
        (KeyCode::Char(character), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            state.editor.insert(&character.to_string());
            None
        }
        _ => None,
    }
}

fn render_state(frame: &mut Frame<'_>, state: &AppState, area: Rect) {
    let lines = vec![
        Line::from(format!("Name: {}", state.character_name)),
        Line::from(format!("Place: {}", state.location)),
        Line::from(format!("Health: {}/{}", state.health.0, state.health.1)),
        Line::from(format!("Stamina: {}/{}", state.stamina.0, state.stamina.1)),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().title(" State ").borders(Borders::ALL)),
        area,
    );
}

fn stream_style(state: StreamState) -> Style {
    match state {
        StreamState::Live => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::ITALIC),
        StreamState::Final => Style::default().fg(Color::White),
        StreamState::Interrupted => Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    }
}

fn stream_label(state: StreamState) -> &'static str {
    match state {
        StreamState::Live => "streaming",
        StreamState::Final => "final",
        StreamState::Interrupted => "interrupted",
    }
}

fn render_story(frame: &mut Frame<'_>, state: &AppState, area: Rect) {
    let mut lines = state
        .transcript
        .iter()
        .map(|row| Line::from(format!("{}: {}", row.speaker, row.text)))
        .collect::<Vec<_>>();
    if let Some(stream) = &state.stream {
        lines.push(Line::from(Span::styled(
            format!("[{}] {}", stream_label(stream.state), stream.text),
            stream_style(stream.state),
        )));
    }
    for tool in &state.tools {
        lines.push(Line::from(vec![
            Span::styled(
                format!("[{}]", tool.state.label()),
                Style::default().fg(tool.state.color()),
            ),
            Span::raw(format!(" {}", tool.name)),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Story ").borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((state.transcript_scroll, 0)),
        area,
    );
}

fn render_input(frame: &mut Frame<'_>, state: &AppState, area: Rect) {
    frame.render_widget(
        Paragraph::new(state.editor.text.as_str())
            .block(Block::default().title(" Input ").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_ui(frame: &mut Frame<'_>, state: &AppState) {
    let area = frame.area();
    let input_height = area.height.min(4);
    let content_height = area.height.saturating_sub(input_height);
    let content = Rect::new(area.x, area.y, area.width, content_height);
    let input = Rect::new(
        area.x,
        area.y.saturating_add(content_height),
        area.width,
        input_height,
    );

    if area.width >= WIDE_LAYOUT_MINIMUM {
        let state_width = area.width.saturating_mul(3) / 10;
        render_state(
            frame,
            state,
            Rect::new(content.x, content.y, state_width, content.height),
        );
        render_story(
            frame,
            state,
            Rect::new(
                content.x.saturating_add(state_width),
                content.y,
                content.width.saturating_sub(state_width),
                content.height,
            ),
        );
    } else {
        let tab_height = content.height.min(3);
        let selected = match state.narrow_page {
            NarrowPage::State => "[State] | Story",
            NarrowPage::Story => "State | [Story]",
        };
        frame.render_widget(
            Paragraph::new(selected).block(Block::default().title(" View ").borders(Borders::ALL)),
            Rect::new(content.x, content.y, content.width, tab_height),
        );
        let page = Rect::new(
            content.x,
            content.y.saturating_add(tab_height),
            content.width,
            content.height.saturating_sub(tab_height),
        );
        match state.narrow_page {
            NarrowPage::State => render_state(frame, state, page),
            NarrowPage::Story => render_story(frame, state, page),
        }
    }

    render_input(frame, state, input);
}

fn render(state: &AppState, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let options = TerminalOptions {
        viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
    };
    let mut terminal = Terminal::with_options(backend, options).expect("test terminal opens");
    terminal
        .draw(|frame| render_ui(frame, state))
        .expect("test frame renders");
    terminal
}

fn text_snapshot(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut rows = Vec::with_capacity(usize::from(area.height));
    for y in area.y..area.bottom() {
        let mut row = String::new();
        for x in area.x..area.right() {
            row.push_str(
                buffer
                    .cell((x, y))
                    .expect("snapshot coordinate is inside the buffer")
                    .symbol(),
            );
        }
        rows.push(row.trim_end().to_owned());
    }
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows.join("\n")
}

fn find_ascii(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let area = buffer.area;
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let fits = needle.chars().enumerate().all(|(offset, character)| {
                let Ok(offset) = u16::try_from(offset) else {
                    return false;
                };
                buffer
                    .cell((x.saturating_add(offset), y))
                    .is_some_and(|cell| cell.symbol() == character.to_string())
            });
            if fits {
                return Some((x, y));
            }
        }
    }
    None
}

trait TerminalOps {
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

struct TerminalSession<T: TerminalOps> {
    ops: T,
    raw_mode: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
    bracketed_paste: bool,
    mouse_capture: bool,
}

impl<T: TerminalOps> TerminalSession<T> {
    fn open(ops: T) -> Result<Self, T::Error> {
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

#[derive(Clone)]
struct RecordingTerminalOps {
    calls: Rc<RefCell<Vec<&'static str>>>,
    fail_on: Option<&'static str>,
}

impl RecordingTerminalOps {
    fn new(fail_on: Option<&'static str>) -> (Self, Rc<RefCell<Vec<&'static str>>>) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        (
            Self {
                calls: Rc::clone(&calls),
                fail_on,
            },
            calls,
        )
    }

    fn call(&mut self, name: &'static str) -> Result<(), &'static str> {
        self.calls.borrow_mut().push(name);
        if self.fail_on == Some(name) {
            Err(name)
        } else {
            Ok(())
        }
    }
}

impl TerminalOps for RecordingTerminalOps {
    type Error = &'static str;

    fn enable_raw_mode(&mut self) -> Result<(), Self::Error> {
        self.call("enable_raw_mode")
    }

    fn disable_raw_mode(&mut self) -> Result<(), Self::Error> {
        self.call("disable_raw_mode")
    }

    fn enter_alternate_screen(&mut self) -> Result<(), Self::Error> {
        self.call("enter_alternate_screen")
    }

    fn leave_alternate_screen(&mut self) -> Result<(), Self::Error> {
        self.call("leave_alternate_screen")
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.call("hide_cursor")
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.call("show_cursor")
    }

    fn enable_bracketed_paste(&mut self) -> Result<(), Self::Error> {
        self.call("enable_bracketed_paste")
    }

    fn disable_bracketed_paste(&mut self) -> Result<(), Self::Error> {
        self.call("disable_bracketed_paste")
    }

    fn enable_mouse_capture(&mut self) -> Result<(), Self::Error> {
        self.call("enable_mouse_capture")
    }

    fn disable_mouse_capture(&mut self) -> Result<(), Self::Error> {
        self.call("disable_mouse_capture")
    }
}

#[test]
fn wide_layout_snapshot_contains_state_story_input_and_tool_styles() {
    let terminal = render(&AppState::sample(), 80, 16);
    let buffer = terminal.backend().buffer();
    assert_eq!(
        text_snapshot(buffer),
        "┌ State ───────────────┐┌ Story ───────────────────────────────────────────────┐\n\
│Name: Aster           ││Mira: It rang before dawn.                            │\n\
│Place: Old Mill       ││[streaming] Dust turns in the amber light...          │\n\
│Health: 17/20         ││[pending] observe_scene                               │\n\
│Stamina: 4/12         ││[succeeded] npc_speak                                 │\n\
│                      ││[rejected] take_crown                                 │\n\
│                      ││[failed] provider_call                                │\n\
│                      ││                                                      │\n\
│                      ││                                                      │\n\
│                      ││                                                      │\n\
│                      ││                                                      │\n\
└──────────────────────┘└──────────────────────────────────────────────────────┘\n\
┌ Input ───────────────────────────────────────────────────────────────────────┐\n\
│Look closer                                                                   │\n\
│                                                                              │\n\
└──────────────────────────────────────────────────────────────────────────────┘"
    );

    for (label, expected_color) in [
        ("[pending]", Color::Yellow),
        ("[succeeded]", Color::Green),
        ("[rejected]", Color::Magenta),
        ("[failed]", Color::Red),
    ] {
        let position = find_ascii(buffer, label).expect("styled tool label is rendered");
        assert_eq!(
            buffer
                .cell(position)
                .expect("tool label starts inside the buffer")
                .fg,
            expected_color
        );
    }
}

#[test]
fn narrow_layout_snapshots_switch_pages_and_keep_input_reachable() {
    let mut state = AppState::sample();
    let story = render(&state, 48, 16);
    assert_eq!(
        text_snapshot(story.backend().buffer()),
        "┌ View ────────────────────────────────────────┐\n\
│State | [Story]                               │\n\
└──────────────────────────────────────────────┘\n\
┌ Story ───────────────────────────────────────┐\n\
│Mira: It rang before dawn.                    │\n\
│[streaming] Dust turns in the amber light...  │\n\
│[pending] observe_scene                       │\n\
│[succeeded] npc_speak                         │\n\
│[rejected] take_crown                         │\n\
│[failed] provider_call                        │\n\
│                                              │\n\
└──────────────────────────────────────────────┘\n\
┌ Input ───────────────────────────────────────┐\n\
│Look closer                                   │\n\
│                                              │\n\
└──────────────────────────────────────────────┘"
    );

    state.narrow_page = NarrowPage::State;
    let status = render(&state, 48, 16);
    assert_eq!(
        text_snapshot(status.backend().buffer()),
        "┌ View ────────────────────────────────────────┐\n\
│[State] | Story                               │\n\
└──────────────────────────────────────────────┘\n\
┌ State ───────────────────────────────────────┐\n\
│Name: Aster                                   │\n\
│Place: Old Mill                               │\n\
│Health: 17/20                                 │\n\
│Stamina: 4/12                                 │\n\
│                                              │\n\
│                                              │\n\
│                                              │\n\
└──────────────────────────────────────────────┘\n\
┌ Input ───────────────────────────────────────┐\n\
│Look closer                                   │\n\
│                                              │\n\
└──────────────────────────────────────────────┘"
    );
    assert!(text_snapshot(story.backend().buffer()).contains("Look closer"));
    assert!(text_snapshot(status.backend().buffer()).contains("Look closer"));
}

#[test]
fn multiline_editor_never_splits_combining_or_emoji_graphemes() {
    let mut editor = InputEditor::with_text("Ae\u{301}👩‍👩‍👧‍👦界");
    assert_eq!(editor.grapheme_count(), 4);
    editor.move_left();
    editor.backspace();
    assert_eq!(editor.text, "Ae\u{301}界");
    assert_eq!(editor.cursor, 2);
    editor.move_left();
    editor.delete();
    assert_eq!(editor.text, "A界");
    editor.insert("\n🌙");
    assert_eq!(editor.text, "A\n🌙界");
    assert_eq!(editor.cursor, 3);
}

#[test]
fn keys_emit_only_ui_intents_and_multiline_shortcuts_edit_locally() {
    let mut state = AppState::sample();
    state.editor = InputEditor::with_text("first");
    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
        None
    );
    state.editor.insert("second");
    assert_eq!(state.editor.text, "first\nsecond");
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        Some(UiIntent::Submit("first\nsecond".to_owned()))
    );
    assert_eq!(state.editor.history, vec!["first\nsecond"]);
    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(UiIntent::Cancel)
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ),
        Some(UiIntent::Quit)
    );
    state.running = false;
    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        None
    );
}

#[test]
fn streaming_row_is_ephemeral_and_final_or_interrupted_is_explicit() {
    let mut state = AppState::sample();
    state.stream = None;
    let committed_rows = state.transcript.len();

    state.append_stream_chunk("The bell");
    state.append_stream_chunk(" answers.");
    assert_eq!(
        state.stream,
        Some(StreamRow {
            text: "The bell answers.".to_owned(),
            state: StreamState::Live,
        })
    );
    assert_eq!(state.transcript.len(), committed_rows);

    state.finish_stream(StreamState::Final);
    assert_eq!(
        state.stream.as_ref().map(|stream| stream.state),
        Some(StreamState::Final)
    );
    state.finish_stream(StreamState::Interrupted);
    assert_eq!(
        state.stream.as_ref().map(|stream| stream.state),
        Some(StreamState::Interrupted)
    );
    assert_eq!(state.transcript.len(), committed_rows);
}

#[test]
fn resize_is_a_pure_projection_and_preserves_interaction_state() {
    let mut state = AppState::sample();
    state.editor.history = vec!["north".to_owned(), "listen".to_owned()];
    state.editor.history_index = Some(1);
    state.editor.cursor = 4;
    state.transcript_scroll = 2;
    let before = state.clone();

    let wide = render(&state, 100, 24);
    let narrow = render(&state, 50, 14);
    let wide_again = render(&state, 100, 24);

    assert_eq!(state, before);
    assert_eq!(
        text_snapshot(wide.backend().buffer()),
        text_snapshot(wide_again.backend().buffer())
    );
    assert_ne!(
        text_snapshot(wide.backend().buffer()),
        text_snapshot(narrow.backend().buffer())
    );
}

#[test]
fn terminal_session_restores_on_drop_partial_init_and_panic_unwind() {
    let full_sequence = vec![
        "enable_raw_mode",
        "enter_alternate_screen",
        "hide_cursor",
        "enable_bracketed_paste",
        "enable_mouse_capture",
        "disable_mouse_capture",
        "disable_bracketed_paste",
        "show_cursor",
        "leave_alternate_screen",
        "disable_raw_mode",
    ];

    let (ops, calls) = RecordingTerminalOps::new(None);
    drop(TerminalSession::open(ops).expect("terminal session initializes"));
    assert_eq!(*calls.borrow(), full_sequence);

    let (ops, calls) = RecordingTerminalOps::new(Some("enable_bracketed_paste"));
    assert!(TerminalSession::open(ops).is_err());
    assert_eq!(
        *calls.borrow(),
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "hide_cursor",
            "enable_bracketed_paste",
            "show_cursor",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );

    let (ops, calls) = RecordingTerminalOps::new(None);
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _session = TerminalSession::open(ops).expect("terminal session initializes");
        panic!("simulated render panic");
    }));
    assert!(unwind.is_err());
    assert_eq!(*calls.borrow(), full_sequence);
}
