use std::{
    cell::RefCell,
    num::NonZeroU32,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use loreloom_core::{
    ActionState, ActorId, AttributeView, CharacterContext, CharacterProfile, ConditionRecord,
    ConditionSource, ConditionView, DisplayName, EntityOrigin, Fixed, LifeState, LongText,
    NoticeKind, ObjectId, Posture, Revision, RuntimePhase, SceneContext, SessionId, ShortText,
    ToolActivity, ToolActivityState, TranscriptItemId, TranscriptItemRecord, TranscriptSpeaker,
    TranscriptState, TranscriptWindow, UiNotice, UiSnapshot, WorldTime,
};
use loreloom_tui::{
    EditorError, InputEditor, MAX_INPUT_BYTES, NarrowPage, RuntimeUiEvent, StreamItem, StreamState,
    TerminalOps, TerminalSession, TuiApp, UiIntent, handle_key, render_ui,
};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

fn parse<T: std::str::FromStr>(value: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    value.parse().expect("fixture identifier")
}

fn name(value: &str) -> DisplayName {
    DisplayName::new(value).expect("display name")
}

fn text(value: &str) -> ShortText {
    ShortText::new(value).expect("short text")
}

fn snapshot() -> UiSnapshot {
    let player = parse::<ActorId>("obj_01890f6a-2b3c-7d4e-8f90-123456789abc");
    let scene = parse::<ObjectId>("obj_01890f6a-2b3d-7d4e-8f90-123456789abc");
    let place = parse::<ObjectId>("obj_01890f6a-2b3e-7d4e-8f90-123456789abc");
    let session = parse::<SessionId>("ses_01890f6a-2b3f-7d4e-8f90-123456789abc");
    UiSnapshot {
        revision: Revision::new(7),
        session_id: session,
        player: CharacterContext {
            actor_id: player,
            revision: Revision::new(7),
            display_name: name("Aster"),
            profile: CharacterProfile {
                summary: text("A patient traveler."),
                values: Vec::new(),
                speaking_style: text("Direct."),
                narrative_tags: Default::default(),
            },
            location_id: place,
            attributes: vec![AttributeView {
                attribute_id: "games.loreloom.demo:attribute/resolve"
                    .parse()
                    .expect("definition id"),
                display_name: name("Resolve"),
                base: Fixed::from_integer(10).expect("base"),
                effective: Fixed::from_integer(12).expect("effective"),
            }],
            resources: vec![loreloom_core::ResourceView {
                resource_id: "games.loreloom.demo:resource/stamina"
                    .parse()
                    .expect("definition id"),
                display_name: name("Stamina"),
                current: Fixed::from_integer(4).expect("current"),
                maximum: Fixed::from_integer(12).expect("maximum"),
            }],
            conditions: vec![ConditionView {
                condition: ConditionRecord {
                    id: parse("obj_01890f6a-2b42-7d4e-8f90-123456789abc"),
                    target_id: player,
                    condition_id: parse("games.loreloom.demo:condition/shivering"),
                    source: ConditionSource::System {
                        source_id: parse("games.loreloom.demo:system/weather"),
                    },
                    stacks: NonZeroU32::MIN,
                    intensity: Fixed::ONE,
                    applied_at: WorldTime::from_ticks(40),
                    expires_at: None,
                    next_periodic_at: None,
                    origin: EntityOrigin::System {
                        source: parse("games.loreloom.demo:system/weather"),
                    },
                },
                display_name: None,
                symptoms: vec![text("Hands tremble.")],
            }],
            inventory: Vec::new(),
            skills: Vec::new(),
            known_facts: Vec::new(),
            goals: Vec::new(),
            life_state: LifeState::Alive,
            action_state: ActionState::Idle,
            posture: Posture::Standing,
        },
        scene: SceneContext {
            scene_id: scene,
            revision: Revision::new(7),
            display_name: name("Old Mill"),
            framing: text("Amber light crosses the mill."),
            place_id: place,
            place_name: name("Bell Room"),
            clock: WorldTime::from_ticks(42),
            visible_actors: Vec::new(),
            recent_events: Vec::new(),
        },
        parameters: Vec::new(),
        active_events: Vec::new(),
        transcript: TranscriptWindow {
            items: vec![
                TranscriptItemRecord {
                    id: parse::<TranscriptItemId>("trn_01890f6a-2b40-7d4e-8f90-123456789abc"),
                    session_id: session,
                    revision: Some(Revision::new(5)),
                    speaker: TranscriptSpeaker::Player {
                        actor_id: player,
                        display_name: name("Aster"),
                    },
                    text: LongText::new("Ask Mira about the bell.").expect("transcript"),
                    state: TranscriptState::Committed,
                    supporting_events: Vec::new(),
                },
                TranscriptItemRecord {
                    id: parse::<TranscriptItemId>("trn_01890f6a-2b41-7d4e-8f90-123456789abc"),
                    session_id: session,
                    revision: Some(Revision::new(6)),
                    speaker: TranscriptSpeaker::Narrator,
                    text: LongText::new("Mira says it rang before dawn.").expect("transcript"),
                    state: TranscriptState::Committed,
                    supporting_events: Vec::new(),
                },
            ],
            before_cursor: None,
        },
        tool_activity: vec![
            tool("observe_scene", ToolActivityState::Pending),
            tool("npc_speak", ToolActivityState::Succeeded),
            tool("take_crown", ToolActivityState::Rejected),
            tool("provider_call", ToolActivityState::Failed),
        ],
        phase: RuntimePhase::Idle,
        can_submit: true,
        can_cancel: false,
        waiting: false,
        notices: vec![UiNotice {
            kind: NoticeKind::Info,
            message: text("Demo ready"),
        }],
        supporting_events: Vec::new(),
    }
}

fn tool(name: &str, state: ToolActivityState) -> ToolActivity {
    ToolActivity {
        call_id: format!("call-{name}"),
        name: name.to_owned(),
        state,
    }
}

fn sample_app() -> TuiApp {
    let mut app = TuiApp::new(snapshot());
    app.editor = InputEditor::with_text("Look closer").expect("input");
    app.stream = Some(StreamItem {
        text: "Dust turns in the amber light...".to_owned(),
        state: StreamState::Live,
    });
    app
}

fn render(app: &TuiApp, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render_ui(frame, app))
        .expect("render");
    terminal
}

fn text_snapshot(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut rows = Vec::with_capacity(usize::from(area.height));
    for y in area.y..area.bottom() {
        let mut row = String::new();
        for x in area.x..area.right() {
            if let Some(cell) = buffer.cell((x, y)) {
                row.push_str(cell.symbol());
            }
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

#[test]
fn product_renderer_is_deterministic_for_wide_and_narrow_layouts() {
    let mut app = sample_app();
    let wide = text_snapshot(render(&app, 80, 18).backend().buffer());
    assert!(wide.contains("┌ State"));
    assert!(wide.contains("┌ Story"));
    assert!(wide.contains("Aster: Ask Mira about the bell."));
    assert!(wide.contains("[streaming] Dust turns in the amber light..."));
    assert!(wide.contains("[pending] observe_scene"));
    assert!(wide.contains("Look closer▏"));
    assert!(wide.contains("rev 7 · Ctrl+C quit"));
    assert_eq!(wide, text_snapshot(render(&app, 80, 18).backend().buffer()));

    let story = text_snapshot(render(&app, 48, 18).backend().buffer());
    assert!(story.contains("State | [Story]"));
    assert!(story.contains("Look closer▏"));
    app.narrow_page = NarrowPage::State;
    let state = text_snapshot(render(&app, 48, 18).backend().buffer());
    assert!(state.contains("[State] | Story"));
    assert!(state.contains("Name: Aster"));
    assert!(state.contains("Condition: Unknown condition Hands tremble."));
    assert!(state.contains("Look closer▏"));
}

#[test]
fn product_renderer_preserves_text_labels_and_distinct_tool_colors() {
    use ratatui::style::Color;

    let terminal = render(&sample_app(), 80, 18);
    let buffer = terminal.backend().buffer();
    for (label, expected) in [
        ("[pending]", Color::Yellow),
        ("[succeeded]", Color::Green),
        ("[rejected]", Color::Magenta),
        ("[failed]", Color::Red),
    ] {
        let position = find_ascii(buffer, label).expect("styled label exists");
        assert_eq!(buffer.cell(position).expect("label cell").fg, expected);
        assert!(text_snapshot(buffer).contains(label));
    }
}

#[test]
fn editor_handles_graphemes_multiline_history_and_atomic_limits() {
    let mut editor = InputEditor::with_text("Ae\u{301}👩‍👩‍👧‍👦界").expect("input");
    assert_eq!(editor.grapheme_count(), 4);
    editor.move_left();
    editor.backspace();
    assert_eq!(editor.text(), "Ae\u{301}界");
    editor.move_left();
    editor.delete();
    editor.insert("\n🌙").expect("insert");
    assert_eq!(editor.text(), "A\n🌙界");

    assert_eq!(editor.submit().as_deref(), Some("A\n🌙界"));
    editor.insert("draft").expect("draft");
    editor.history_previous();
    assert_eq!(editor.text(), "A\n🌙界");
    editor.history_next();
    assert_eq!(editor.text(), "draft");

    let mut full = InputEditor::with_text("a".repeat(MAX_INPUT_BYTES)).expect("max input");
    assert_eq!(full.insert("b"), Err(EditorError::TooLong));
    assert_eq!(full.text().len(), MAX_INPUT_BYTES);

    let mut combining = InputEditor::with_text("e").expect("base grapheme");
    combining.insert("\u{301}").expect("combining mark");
    assert_eq!(combining.grapheme_count(), 1);
    assert_eq!(combining.cursor(), 1);
    combining.backspace();
    assert_eq!(combining.text(), "");

    let mut crlf = InputEditor::with_text("first\r\nsecond").expect("CRLF input");
    crlf.move_home();
    assert_eq!(crlf.cursor(), 6);
    crlf.move_up();
    assert_eq!(crlf.cursor(), 0);
}

#[test]
fn key_mapping_emits_ui_intents_without_constructing_world_commands() {
    let mut app = sample_app();
    app.editor = InputEditor::with_text("first").expect("input");
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
        None
    );
    app.editor.insert("second").expect("second line");
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(UiIntent::Submit("first\nsecond".to_owned()))
    );
    app.snapshot.can_cancel = true;
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(UiIntent::Cancel)
    );
    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ),
        Some(UiIntent::Quit)
    );
}

#[test]
fn runtime_events_change_only_snapshot_or_ephemeral_stream_state() {
    let mut app = sample_app();
    let transcript = app.snapshot.transcript.clone();
    app.apply_runtime_event(RuntimeUiEvent::StreamStarted);
    app.apply_runtime_event(RuntimeUiEvent::StreamChunk("The bell".to_owned()));
    app.apply_runtime_event(RuntimeUiEvent::StreamChunk(" answers.".to_owned()));
    assert_eq!(app.snapshot.transcript, transcript);
    assert_eq!(
        app.stream.as_ref().map(|stream| stream.text.as_str()),
        Some("The bell answers.")
    );
    app.apply_runtime_event(RuntimeUiEvent::StreamFinished(StreamState::Interrupted));
    assert_eq!(
        app.stream.as_ref().map(|stream| stream.state),
        Some(StreamState::Interrupted)
    );

    let editor = app.editor.clone();
    let mut next = snapshot();
    next.revision = Revision::new(8);
    app.apply_runtime_event(RuntimeUiEvent::Snapshot(Box::new(next)));
    assert_eq!(app.snapshot.revision, Revision::new(8));
    assert_eq!(app.editor, editor);
}

#[derive(Clone)]
struct RecordingOps {
    calls: Rc<RefCell<Vec<&'static str>>>,
    fail_on: Option<&'static str>,
}

impl RecordingOps {
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

impl TerminalOps for RecordingOps {
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
fn terminal_session_restores_normal_partial_and_unwind_paths_in_reverse_order() {
    let complete = vec![
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
    let (ops, calls) = RecordingOps::new(None);
    drop(TerminalSession::open(ops).expect("open"));
    assert_eq!(*calls.borrow(), complete);

    let (ops, calls) = RecordingOps::new(Some("enable_bracketed_paste"));
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

    let (ops, calls) = RecordingOps::new(None);
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _session = TerminalSession::open(ops).expect("open");
        panic!("render panic");
    }));
    assert!(unwind.is_err());
    assert_eq!(*calls.borrow(), complete);
}
