use std::{
    cell::RefCell,
    num::NonZeroU32,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use loreloom_core::{
    ActionState, ActorId, AttributeView, CharacterContext, CharacterProfile, ConditionRecord,
    ConditionSource, ConditionView, DisplayName, EntityOrigin, Fixed, LifeState, LongText, ModId,
    ModPackageStatus, ModPackageView, NoticeKind, ObjectId, PackageCatalogView, Posture, Revision,
    RuntimePhase, SceneContext, SessionId, ShortText, ToolActivity, ToolActivityState,
    TranscriptItemId, TranscriptItemRecord, TranscriptSpeaker, TranscriptState, TranscriptWindow,
    UiNotice, UiSnapshot, WorldPackageView, WorldTime,
};
use loreloom_tui::{
    EditorError, InputEditor, MAX_INPUT_BYTES, NarrowPage, RuntimeUiEvent, TerminalOps,
    TerminalSession, TuiApp, TuiOverlay, UiIntent, handle_key, handle_mouse, handle_paste,
    render_ui,
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
            adjacent_places: Vec::new(),
            clock: WorldTime::from_ticks(42),
            visible_actors: Vec::new(),
            recent_events: Vec::new(),
        },
        parameters: Vec::new(),
        active_events: Vec::new(),
        packages: PackageCatalogView {
            world: WorldPackageView {
                world_id: ModId::parse("games.loreloom.demo").expect("world ID"),
                version: "1.0.0".parse().expect("world version"),
            },
            mods: vec![
                ModPackageView {
                    mod_id: ModId::parse("games.loreloom.weather").expect("enabled Mod ID"),
                    version: "1.2.0".parse().expect("enabled Mod version"),
                    status: ModPackageStatus::Enabled,
                    dependency_count: 1,
                },
                ModPackageView {
                    mod_id: ModId::parse("games.loreloom.characters").expect("installed Mod ID"),
                    version: "2.0.0".parse().expect("installed Mod version"),
                    status: ModPackageStatus::Installed,
                    dependency_count: 0,
                },
            ],
            unavailable_installed: 1,
        },
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
            tool_with_code("take_crown", ToolActivityState::Rejected, "invalid_input"),
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
        code: None,
    }
}

fn tool_with_code(name: &str, state: ToolActivityState, code: &str) -> ToolActivity {
    ToolActivity {
        code: Some(code.to_owned()),
        ..tool(name, state)
    }
}

fn sample_app() -> TuiApp {
    let mut app = TuiApp::new(snapshot());
    app.editor = InputEditor::with_text("Look closer").expect("input");
    app.working_phase = Some(RuntimePhase::NarratorThinking);
    app
}

fn render(app: &TuiApp, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut app = app.clone();
    render_mut(&mut app, width, height)
}

fn render_mut(app: &mut TuiApp, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render_ui(frame, app))
        .expect("render");
    terminal
}

fn wheel(kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }
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
    let wide_terminal = render(&app, 80, 18);
    let wide = text_snapshot(wide_terminal.backend().buffer());
    assert_eq!(
        wide,
        include_str!("../../../tests/data/tui/wide-thinking.txt").trim_end_matches('\n')
    );
    assert!(wide.contains("LORELOOM  Old Mill · Bell Room"));
    assert!(wide.contains("STORY"));
    assert!(wide.contains("› Ask Mira about the bell."));
    assert!(wide.contains("Mira says it rang before dawn."));
    assert!(!wide.contains("Narrator:"));
    assert!(wide.contains("Narrator is thinking…"));
    assert!(wide.contains("observe_scene  running"));
    assert!(wide.contains("Look closer▏"));
    assert!(wide.contains("Esc cancel"));
    assert!(!wide.contains("rev 7"));
    assert!(!wide.contains("r7"));
    assert!(
        find_ascii(wide_terminal.backend().buffer(), "Look closer").is_some_and(|(x, _)| x > 24),
        "the wide composer stays in the right narrative pane"
    );
    assert_eq!(wide, text_snapshot(render(&app, 80, 18).backend().buffer()));

    let story_terminal = render(&app, 48, 18);
    let story = text_snapshot(story_terminal.backend().buffer());
    assert_eq!(
        story,
        include_str!("../../../tests/data/tui/narrow-story-thinking.txt").trim_end_matches('\n')
    );
    assert!(story.contains("STATE   STORY   Tab to switch"));
    let story_tab = find_ascii(story_terminal.backend().buffer(), "STORY").expect("story tab");
    assert_eq!(
        story_terminal
            .backend()
            .buffer()
            .cell(story_tab)
            .expect("story tab cell")
            .fg,
        ratatui::style::Color::Cyan
    );
    assert!(story.contains("Look closer▏"));
    app.narrow_page = NarrowPage::State;
    let state = text_snapshot(render(&app, 48, 18).backend().buffer());
    assert_eq!(
        state,
        include_str!("../../../tests/data/tui/narrow-state-thinking.txt").trim_end_matches('\n')
    );
    assert!(state.contains("Aster"));
    assert!(state.contains("STATUS"));
    assert!(state.contains("Unknown condition · Hands tremble."));
    assert!(state.contains("Look closer▏"));

    app.working_phase = None;
    app.editor = InputEditor::with_text("first\nsecond").expect("multiline input");
    let multiline = text_snapshot(render(&app, 80, 18).backend().buffer());
    assert!(multiline.contains("│› first"));
    assert!(multiline.contains("│  second▏"));
}

#[test]
fn accepted_input_is_rendered_before_thinking_and_reconciled_by_snapshot() {
    let mut app = TuiApp::new(snapshot());
    app.snapshot.tool_activity.clear();
    app.snapshot.notices.clear();
    let committed_items = app.snapshot.transcript.items.len();

    app.show_submitted_input("hello".to_owned());
    app.apply_runtime_event(RuntimeUiEvent::PhaseChanged(RuntimePhase::NarratorThinking));

    assert_eq!(app.snapshot.transcript.items.len(), committed_items);
    let pending = text_snapshot(render(&app, 80, 18).backend().buffer());
    let input_position = pending.find("› hello").expect("pending player input");
    let thinking_position = pending.find("Narrator is thinking…").expect("thinking row");
    assert!(input_position < thinking_position);

    let mut next = app.snapshot.clone();
    next.revision = Revision::new(8);
    next.phase = RuntimePhase::NarratorThinking;
    let mut committed = next.transcript.items[0].clone();
    committed.id = parse("trn_01890f6a-2b44-7d4e-8f90-123456789abc");
    committed.revision = Some(Revision::new(8));
    committed.text = LongText::new("hello").expect("committed player input");
    next.transcript.items.push(committed);
    app.apply_runtime_event(RuntimeUiEvent::Snapshot(Box::new(next)));

    let reconciled = text_snapshot(render(&app, 80, 18).backend().buffer());
    assert_eq!(reconciled.matches("› hello").count(), 1);

    let mut failed_app = TuiApp::new(snapshot());
    failed_app.show_submitted_input("not committed".to_owned());
    let mut failed = failed_app.snapshot.clone();
    failed.phase = RuntimePhase::Failed;
    failed_app.apply_runtime_event(RuntimeUiEvent::Snapshot(Box::new(failed)));
    let failed = text_snapshot(render(&failed_app, 80, 18).backend().buffer());
    assert!(!failed.contains("not committed"));
}

#[test]
fn tool_activity_updates_immediately_and_settles_before_final_narration() {
    let mut app = TuiApp::new(snapshot());
    app.snapshot.tool_activity.clear();
    app.snapshot.notices.clear();
    let committed = app.snapshot.transcript.clone();

    app.show_submitted_input("hello".to_owned());
    app.apply_runtime_event(RuntimeUiEvent::PhaseChanged(RuntimePhase::NarratorThinking));
    app.apply_runtime_event(RuntimeUiEvent::ToolActivityChanged(vec![tool(
        "narrator.create_scene",
        ToolActivityState::Pending,
    )]));

    assert_eq!(app.snapshot.transcript, committed);
    let pending = text_snapshot(render(&app, 80, 28).backend().buffer());
    let input_position = pending.find("› hello").expect("pending player input");
    let tool_position = pending
        .find("narrator.create_scene  running")
        .expect("live tool activity");
    let thinking_position = pending.find("Narrator is thinking…").expect("thinking row");
    assert!(input_position < tool_position);
    assert!(tool_position < thinking_position);

    let settled = tool("narrator.create_scene", ToolActivityState::Succeeded);
    app.apply_runtime_event(RuntimeUiEvent::ToolActivityChanged(vec![settled.clone()]));
    let settled_frame = text_snapshot(render(&app, 80, 28).backend().buffer());
    assert!(settled_frame.contains("narrator.create_scene  done"));
    assert!(!settled_frame.contains("narrator.create_scene  running"));

    let mut final_snapshot = app.snapshot.clone();
    final_snapshot.revision = Revision::new(8);
    final_snapshot.phase = RuntimePhase::Completed;
    final_snapshot.tool_activity = vec![settled];
    let mut player = final_snapshot.transcript.items[0].clone();
    player.id = parse("trn_01890f6a-2b45-7d4e-8f90-123456789abc");
    player.revision = Some(Revision::new(8));
    player.text = LongText::new("hello").expect("committed player input");
    let mut narrator = final_snapshot.transcript.items[1].clone();
    narrator.id = parse("trn_01890f6a-2b46-7d4e-8f90-123456789abc");
    narrator.revision = Some(Revision::new(8));
    narrator.text = LongText::new("Final narration.").expect("final narration");
    final_snapshot.transcript.items.extend([player, narrator]);
    app.apply_runtime_event(RuntimeUiEvent::Snapshot(Box::new(final_snapshot)));

    let final_frame = text_snapshot(render(&app, 80, 28).backend().buffer());
    let input_position = final_frame.find("› hello").expect("committed player input");
    let tool_position = final_frame
        .find("narrator.create_scene  done")
        .expect("settled tool activity");
    let narration_position = final_frame
        .find("Final narration.")
        .expect("final narration");
    assert!(input_position < tool_position);
    assert!(tool_position < narration_position);
}

#[test]
fn transcript_follows_latest_and_scrolls_by_page_or_mouse_with_wrapped_bounds() {
    let mut app = sample_app();
    app.working_phase = None;
    app.snapshot.tool_activity.clear();
    app.snapshot.notices.clear();
    let template = app.snapshot.transcript.items[1].clone();
    app.snapshot.transcript.items = (0..20)
        .map(|index| {
            let mut item = template.clone();
            item.text =
                LongText::new(format!("Narration {index}")).expect("bounded transcript fixture");
            item
        })
        .collect();

    let latest = text_snapshot(render_mut(&mut app, 80, 18).backend().buffer());
    assert!(latest.contains("Narration 19"));
    assert!(!latest.contains("Narration 0"));
    assert_eq!(app.transcript_scroll, 0);
    assert!(app.transcript_scroll_max > app.transcript_page_rows);

    handle_key(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    assert_eq!(
        app.transcript_scroll,
        app.transcript_page_rows.saturating_sub(1)
    );
    let older = text_snapshot(render_mut(&mut app, 80, 18).backend().buffer());
    assert!(older.contains("Narration 10"));
    assert!(!older.contains("Narration 19"));

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
    );
    assert_eq!(app.transcript_scroll, 0);
    handle_mouse(&mut app, wheel(MouseEventKind::ScrollUp));
    assert_eq!(app.transcript_scroll, 3);
    handle_mouse(&mut app, wheel(MouseEventKind::ScrollDown));
    assert_eq!(app.transcript_scroll, 0);

    for _ in 0..20 {
        handle_key(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    }
    assert_eq!(app.transcript_scroll, app.transcript_scroll_max);
    handle_mouse(&mut app, wheel(MouseEventKind::ScrollDown));
    assert_eq!(
        app.transcript_scroll,
        app.transcript_scroll_max.saturating_sub(3)
    );

    app.scroll_down_lines(u16::MAX);
    let mut next = app.snapshot.clone();
    let mut appended = template;
    appended.text = LongText::new("Newest committed narration").expect("latest transcript");
    next.transcript.items.push(appended);
    app.apply_runtime_event(RuntimeUiEvent::Snapshot(Box::new(next)));
    assert_eq!(app.transcript_scroll, 0);
    let updated = text_snapshot(render_mut(&mut app, 80, 18).backend().buffer());
    assert!(updated.contains("Newest committed narration"));

    handle_mouse(&mut app, wheel(MouseEventKind::ScrollUp));
    assert_eq!(app.transcript_scroll, 3);
    let preserved = app.snapshot.clone();
    app.apply_runtime_event(RuntimeUiEvent::Snapshot(Box::new(preserved)));
    assert_eq!(
        app.transcript_scroll, 3,
        "a committed snapshot must not reset a user's local reading position"
    );
}

#[test]
fn product_renderer_preserves_text_labels_and_distinct_tool_colors() {
    use ratatui::style::Color;

    let terminal = render(&sample_app(), 80, 18);
    let buffer = terminal.backend().buffer();
    for (label, expected) in [
        ("◌", Color::Yellow),
        ("✓", Color::DarkGray),
        ("!", Color::Magenta),
        ("×", Color::Red),
    ] {
        let position = find_ascii(buffer, label).expect("styled label exists");
        assert_eq!(buffer.cell(position).expect("label cell").fg, expected);
        assert!(text_snapshot(buffer).contains(label));
    }
    let text = text_snapshot(buffer);
    for label in ["running", "done", "rejected", "failed"] {
        assert!(text.contains(label));
    }
    assert!(text.contains("invalid_input"));
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
    app.working_phase = None;
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
fn mods_overlay_is_read_only_scrollable_and_does_not_steal_plain_input() {
    let mut app = sample_app();
    app.working_phase = None;
    let original_editor = app.editor.clone();
    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)
        ),
        None
    );
    assert_eq!(app.overlay, Some(TuiOverlay::Mods));
    let overlay = text_snapshot(render_mut(&mut app, 80, 22).backend().buffer());
    assert!(overlay.contains("MODS · Alt+M / F2 / Esc close"));
    assert!(overlay.contains("games.loreloom.demo"));
    assert!(overlay.contains("ENABLED (1)"));
    assert!(overlay.contains("games.loreloom.weather"));
    assert!(overlay.contains("INSTALLED, NOT ENABLED (1)"));
    assert!(overlay.contains("games.loreloom.characters"));
    assert!(overlay.contains("1 installed candidate(s) unavailable"));
    assert!(!overlay.contains("content_hash"));

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    );
    handle_paste(&mut app, "must not enter the editor").expect("ignored overlay paste");
    assert_eq!(app.editor, original_editor);
    app.snapshot.can_cancel = true;
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        None
    );
    assert_eq!(app.overlay, None);

    handle_key(&mut app, KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    assert_eq!(app.overlay, Some(TuiOverlay::Mods));
    handle_key(&mut app, KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    assert_eq!(app.overlay, None);

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE),
    );
    assert!(app.editor.text().ends_with('m'));
}

#[test]
fn mods_overlay_uses_independent_keyboard_and_mouse_scroll() {
    let mut app = sample_app();
    for index in 0..20 {
        app.snapshot.packages.mods.push(ModPackageView {
            mod_id: ModId::parse(format!("games.loreloom.extra-{index:02}")).expect("extra Mod ID"),
            version: "1.0.0".parse().expect("extra Mod version"),
            status: ModPackageStatus::Installed,
            dependency_count: 0,
        });
    }
    app.toggle_mods_overlay();
    let transcript_scroll = app.transcript_scroll;
    let first = text_snapshot(render_mut(&mut app, 60, 14).backend().buffer());
    assert!(first.contains("games.loreloom.demo"));
    assert!(app.mods_scroll_max > 0);

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
    );
    assert!(app.mods_scroll > 0);
    let keyboard_scroll = app.mods_scroll;
    handle_mouse(&mut app, wheel(MouseEventKind::ScrollDown));
    assert!(app.mods_scroll >= keyboard_scroll);
    assert_eq!(app.transcript_scroll, transcript_scroll);
    let later = text_snapshot(render_mut(&mut app, 60, 14).backend().buffer());
    assert_ne!(first, later);

    handle_mouse(&mut app, wheel(MouseEventKind::ScrollUp));
    assert!(app.mods_scroll <= keyboard_scroll);
}

#[test]
fn runtime_events_change_only_snapshot_or_ephemeral_thinking_state() {
    let mut app = sample_app();
    app.working_phase = None;
    let transcript = app.snapshot.transcript.clone();
    app.apply_runtime_event(RuntimeUiEvent::PhaseChanged(RuntimePhase::NpcThinking));
    assert_eq!(app.snapshot.transcript, transcript);
    assert_eq!(app.working_phase, Some(RuntimePhase::NpcThinking));
    let frame = app.spinner_frame;
    app.tick_spinner();
    assert_ne!(app.spinner_frame, frame);
    app.transcript_scroll = 3;
    app.apply_runtime_event(RuntimeUiEvent::ToolActivityChanged(vec![tool(
        "observe_scene",
        ToolActivityState::Pending,
    )]));
    assert_eq!(app.snapshot.transcript, transcript);
    assert_eq!(app.transcript_scroll, 3);

    let editor = app.editor.clone();
    let mut next = snapshot();
    next.revision = Revision::new(8);
    next.phase = RuntimePhase::Completed;
    app.apply_runtime_event(RuntimeUiEvent::Snapshot(Box::new(next)));
    assert_eq!(app.snapshot.revision, Revision::new(8));
    assert_eq!(app.editor, editor);
    assert_eq!(app.working_phase, None);
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
