use loreloom_core::{
    ActionState, Fixed, LifeState, NoticeKind, ParameterValue, Posture, RuntimePhase, ToolActivity,
    ToolActivityState, TranscriptSpeaker, TranscriptState, UiSnapshot, WorldTime,
};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

use crate::{NarrowPage, TuiApp};

pub const WIDE_LAYOUT_MINIMUM: u16 = 80;

const HEADER_HEIGHT: u16 = 2;
const COMPOSER_HEIGHT: u16 = 4;
const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

pub fn render_ui(frame: &mut Frame<'_>, app: &mut TuiApp) {
    render_ui_with_state_width(frame, app, 30);
}

pub(crate) fn render_ui_with_state_width(
    frame: &mut Frame<'_>,
    app: &mut TuiApp,
    state_width_percent: u16,
) {
    let area = frame.area();
    let header_height = area.height.min(HEADER_HEIGHT);
    let footer_height = u16::from(area.height > header_height);
    let main_height = area
        .height
        .saturating_sub(header_height)
        .saturating_sub(footer_height);
    let header = Rect::new(area.x, area.y, area.width, header_height);
    let main = Rect::new(
        area.x,
        area.y.saturating_add(header_height),
        area.width,
        main_height,
    );
    let footer = Rect::new(
        area.x,
        main.y.saturating_add(main.height),
        area.width,
        footer_height,
    );

    render_header(frame, app, header);
    if area.width >= WIDE_LAYOUT_MINIMUM {
        render_wide(frame, app, main, state_width_percent);
    } else {
        render_narrow(frame, app, main);
    }
    render_footer(frame, app, footer, area.width < WIDE_LAYOUT_MINIMUM);
}

fn render_header(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    if area.height == 0 {
        return;
    }
    let left_width = area.width.saturating_mul(2) / 3;
    let left = Rect::new(area.x.saturating_add(1), area.y, left_width, 1);
    let right = Rect::new(
        area.x.saturating_add(left_width),
        area.y,
        area.width.saturating_sub(left_width).saturating_sub(1),
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "LORELOOM",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  {} · {}",
                    app.snapshot.scene.display_name, app.snapshot.scene.place_name
                ),
                Style::default().fg(MUTED),
            ),
        ])),
        left,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                header_phase_label(app.effective_phase()),
                Style::default().fg(if app.working_phase.is_some() {
                    Color::Yellow
                } else {
                    Color::Green
                }),
            ),
            Span::styled(
                format!("  rev {}", app.snapshot.revision),
                Style::default().fg(MUTED),
            ),
        ]))
        .alignment(Alignment::Right),
        right,
    );
    if area.height > 1 {
        frame.render_widget(
            Paragraph::new("─".repeat(usize::from(area.width))).style(Style::default().fg(MUTED)),
            Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
        );
    }
}

fn render_wide(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect, state_width_percent: u16) {
    let state_width = area.width.saturating_mul(state_width_percent) / 100;
    let sidebar = Rect::new(area.x, area.y, state_width, area.height);
    let right = Rect::new(
        area.x.saturating_add(state_width),
        area.y,
        area.width.saturating_sub(state_width),
        area.height,
    );
    let composer_height = right.height.min(COMPOSER_HEIGHT);
    let story = Rect::new(
        right.x.saturating_add(1),
        right.y,
        right.width.saturating_sub(2),
        right.height.saturating_sub(composer_height),
    );
    let composer = Rect::new(
        right.x.saturating_add(1),
        right
            .y
            .saturating_add(right.height.saturating_sub(composer_height)),
        right.width.saturating_sub(2),
        composer_height,
    );

    render_state(frame, &app.snapshot, sidebar, true);
    render_story(frame, app, story);
    render_input(frame, app, composer);
}

fn render_narrow(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
    let composer_height = area.height.min(COMPOSER_HEIGHT);
    let content_height = area.height.saturating_sub(composer_height);
    let tab_height = u16::from(content_height > 0);
    let tabs = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        tab_height,
    );
    let page = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(tab_height),
        area.width.saturating_sub(2),
        content_height.saturating_sub(tab_height),
    );
    let composer = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(content_height),
        area.width.saturating_sub(2),
        composer_height,
    );

    if tab_height > 0 {
        let (state, story) = match app.narrow_page {
            NarrowPage::State => (
                Span::styled(
                    "STATE",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("STORY", Style::default().fg(MUTED)),
            ),
            NarrowPage::Story => (
                Span::styled("STATE", Style::default().fg(MUTED)),
                Span::styled(
                    "STORY",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
            ),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                state,
                Span::raw("   "),
                story,
                Span::styled("   Tab to switch", Style::default().fg(MUTED)),
            ])),
            tabs,
        );
    }
    match app.narrow_page {
        NarrowPage::State => render_state(frame, &app.snapshot, page, false),
        NarrowPage::Story => render_story(frame, app, page),
    }
    render_input(frame, app, composer);
}

fn render_state(frame: &mut Frame<'_>, snapshot: &UiSnapshot, area: Rect, separated: bool) {
    let player = &snapshot.player;
    let mut lines = vec![Line::from(Span::styled(
        player.display_name.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if separated {
        lines.extend([
            Line::from(Span::styled(
                format!(
                    "{} · {}",
                    snapshot.scene.display_name, snapshot.scene.place_name
                ),
                Style::default().fg(ACCENT),
            )),
            Line::from(Span::styled(
                format_world_time(snapshot.scene.clock),
                Style::default().fg(MUTED),
            )),
        ]);
    }
    section(&mut lines, "STATUS");
    lines.push(Line::from(format!(
        "{} · {} · {}",
        life_label(player.life_state),
        posture_label(player.posture),
        action_label(player.action_state)
    )));

    if !player.resources.is_empty() {
        section(&mut lines, "RESOURCES");
        for resource in &player.resources {
            lines.push(Line::from(vec![
                Span::raw(format!("{}  ", resource.display_name)),
                Span::styled(
                    resource_bar(resource.current, resource.maximum, 7),
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    format!(
                        "  {}/{}",
                        format_fixed(resource.current),
                        format_fixed(resource.maximum)
                    ),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
    }
    if !player.attributes.is_empty() {
        section(&mut lines, "ATTRIBUTES");
        for attribute in &player.attributes {
            let base = if attribute.base != attribute.effective {
                format!("  base {}", format_fixed(attribute.base))
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::raw(format!("{}  ", attribute.display_name)),
                Span::styled(
                    format_fixed(attribute.effective),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(base, Style::default().fg(MUTED)),
            ]));
        }
    }
    if !player.conditions.is_empty() {
        section(&mut lines, "CONDITIONS");
        for condition in &player.conditions {
            let name = condition
                .display_name
                .as_ref()
                .map_or("Unknown condition", loreloom_core::DisplayName::as_str);
            let symptom = condition
                .symptoms
                .first()
                .map_or("", loreloom_core::ShortText::as_str);
            let detail = if symptom.is_empty() {
                String::new()
            } else {
                format!(" · {symptom}")
            };
            lines.push(Line::from(vec![
                Span::styled("◇ ", Style::default().fg(Color::Yellow)),
                Span::raw(name.to_owned()),
                Span::styled(detail, Style::default().fg(MUTED)),
            ]));
        }
    }
    if !player.inventory.is_empty() {
        section(&mut lines, "INVENTORY");
        for item in &player.inventory {
            lines.push(Line::from(format!(
                "• {}  ×{}",
                item.display_name,
                item.item.stack.0.get()
            )));
        }
    }
    if !player.skills.is_empty() {
        section(&mut lines, "SKILLS");
        for skill in &player.skills {
            lines.push(Line::from(vec![
                Span::styled(
                    if skill.available { "◆ " } else { "◇ " },
                    Style::default().fg(if skill.available { ACCENT } else { MUTED }),
                ),
                Span::styled(
                    skill.display_name.to_string(),
                    Style::default().fg(if skill.available { Color::Reset } else { MUTED }),
                ),
            ]));
        }
    }
    if !player.goals.is_empty() {
        section(&mut lines, "GOALS");
        for goal in &player.goals {
            lines.push(Line::from(format!("○ {}", goal.description)));
        }
    }
    if snapshot.parameters.iter().any(|set| !set.values.is_empty()) {
        section(&mut lines, "WORLD");
        for set in &snapshot.parameters {
            for value in &set.values {
                lines.push(Line::from(format!(
                    "{}  {}",
                    value.display_name,
                    format_parameter(&value.value)
                )));
            }
        }
    }
    if !snapshot.active_events.is_empty() {
        section(&mut lines, "CHOICES");
        for event in &snapshot.active_events {
            lines.push(Line::from(Span::styled(
                event.display_name.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for (index, option) in event.options.iter().enumerate() {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}  ", index + 1),
                        Style::default().fg(if option.enabled { ACCENT } else { MUTED }),
                    ),
                    Span::styled(
                        option.display_name.to_string(),
                        Style::default().fg(if option.enabled { Color::Reset } else { MUTED }),
                    ),
                ]));
            }
        }
    }

    let block = if separated {
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(MUTED))
    } else {
        Block::default()
    };
    let target = if separated {
        Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(1),
            area.height,
        )
    } else {
        area
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        target,
    );
}

fn render_story(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let body = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    let mut lines = Vec::new();
    let tool_activity = app.tool_activity();
    let tool_insert_before = if app.working_phase.is_none() && !tool_activity.is_empty() {
        app.snapshot
            .transcript
            .items
            .iter()
            .rposition(|item| matches!(item.speaker, TranscriptSpeaker::Narrator))
    } else {
        None
    };
    for (index, item) in app.snapshot.transcript.items.iter().enumerate() {
        if tool_insert_before == Some(index) {
            push_tool_activity(&mut lines, tool_activity);
        }
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        let text_style = if matches!(item.state, TranscriptState::Interrupted) {
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC)
        } else {
            Style::default()
        };
        let mut text_lines = item.text.as_str().lines();
        let first = text_lines.next().unwrap_or_default();
        match &item.speaker {
            TranscriptSpeaker::Player { .. } => lines.push(Line::from(vec![
                Span::styled(
                    "› ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(first.to_owned(), text_style),
            ])),
            TranscriptSpeaker::Narrator => {
                lines.push(Line::from(Span::styled(first.to_owned(), text_style)));
            }
            TranscriptSpeaker::Actor { display_name, .. } => lines.push(Line::from(vec![
                Span::styled(
                    display_name.to_string(),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {first}"), text_style),
            ])),
            TranscriptSpeaker::System => lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(MUTED)),
                Span::styled(first.to_owned(), text_style.fg(MUTED)),
            ])),
        }
        for continuation in text_lines {
            lines.push(Line::from(Span::styled(
                format!("  {continuation}"),
                text_style,
            )));
        }
    }
    if let Some(input) = app.pending_submission_text() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        let mut text_lines = input.lines();
        let first = text_lines.next().unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(
                "› ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(first.to_owned()),
        ]));
        for continuation in text_lines {
            lines.push(Line::from(format!("  {continuation}")));
        }
    }
    if tool_insert_before.is_none() {
        push_tool_activity(&mut lines, tool_activity);
    }
    if let Some(phase) = app.working_phase {
        if !lines.is_empty() && tool_activity.is_empty() {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            format!("{} {}…", spinner(app.spinner_frame), phase_label(phase)),
            Style::default().fg(ACCENT).add_modifier(Modifier::ITALIC),
        )));
    }
    for notice in &app.snapshot.notices {
        let (symbol, color) = match notice.kind {
            NoticeKind::Info => ("i", ACCENT),
            NoticeKind::Warning => ("!", Color::Yellow),
            NoticeKind::Error => ("×", Color::Red),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{symbol} "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(notice.message.to_string(), Style::default().fg(color)),
        ]));
    }
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let visual_rows = paragraph.line_count(body.width);
    let maximum = visual_rows.saturating_sub(usize::from(body.height));
    let maximum = u16::try_from(maximum).unwrap_or(u16::MAX);
    app.update_transcript_layout(maximum, body.height);
    let scroll_hint = if app.transcript_scroll_max == 0 {
        if app.snapshot.transcript.before_cursor.is_some() {
            "   ↑ earlier history outside window"
        } else {
            ""
        }
    } else if app.transcript_scroll == 0 {
        "   ↑ older"
    } else if app.transcript_scroll == app.transcript_scroll_max {
        "   ↓ latest"
    } else {
        "   ↑ older · ↓ latest"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "STORY",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            ),
            Span::styled(scroll_hint, Style::default().fg(MUTED)),
        ])),
        Rect::new(area.x, area.y, area.width, 1),
    );
    frame.render_widget(paragraph.scroll((app.transcript_top_offset(), 0)), body);
}

fn render_input(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let ready = app.can_submit();
    let border = if ready { ACCENT } else { MUTED };
    let title = if ready {
        " Message "
    } else {
        " Working · Esc to cancel "
    };
    let editor = app.editor.text_with_cursor();
    let mut input_lines = editor.split('\n');
    let first = input_lines.next().unwrap_or_default();
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "› ",
            Style::default().fg(border).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            first.to_owned(),
            Style::default().fg(if ready { Color::Reset } else { MUTED }),
        ),
    ])];
    lines.extend(input_lines.map(|line| {
        Line::from(Span::styled(
            format!("  {line}"),
            Style::default().fg(if ready { Color::Reset } else { MUTED }),
        ))
    }));
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(Span::styled(title, Style::default().fg(border)))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, app: &TuiApp, area: Rect, narrow: bool) {
    if area.height == 0 {
        return;
    }
    let help = if narrow {
        " · Tab · PgUp/PgDn · ^C"
    } else if app.can_cancel() {
        "  ·  Esc cancel  ·  PgUp/PgDn scroll  ·  ^C quit"
    } else {
        "  ·  Enter send  ·  Alt+Enter newline  ·  PgUp/PgDn scroll  ·  ^C quit"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                if narrow {
                    header_phase_label(app.effective_phase())
                } else {
                    phase_label(app.effective_phase())
                },
                Style::default().fg(if app.working_phase.is_some() {
                    Color::Yellow
                } else {
                    Color::Green
                }),
            ),
            Span::styled(
                format!("  ·  rev {}", app.snapshot.revision),
                Style::default().fg(MUTED),
            ),
            Span::styled(help, Style::default().fg(MUTED)),
        ]))
        .alignment(Alignment::Center),
        area,
    );
}

fn push_tool_activity(lines: &mut Vec<Line<'static>>, activity: &[ToolActivity]) {
    if activity.is_empty() {
        return;
    }
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines.extend(
        activity
            .iter()
            .map(|tool| tool_line(tool.name.as_str(), tool.state, tool.code.as_deref())),
    );
}

fn section(lines: &mut Vec<Line<'static>>, title: &'static str) {
    lines.push(Line::from(Span::styled(
        title,
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));
}

fn tool_line(name: &str, state: ToolActivityState, code: Option<&str>) -> Line<'static> {
    let (symbol, label, color) = match state {
        ToolActivityState::Pending => ("◌", "running", Color::Yellow),
        ToolActivityState::Succeeded => ("✓", "done", MUTED),
        ToolActivityState::Rejected => ("!", "rejected", Color::Magenta),
        ToolActivityState::Failed => ("×", "failed", Color::Red),
    };
    let mut spans = vec![
        Span::styled(format!("{symbol} "), Style::default().fg(color)),
        Span::styled(name.to_owned(), Style::default().fg(color)),
        Span::styled(format!("  {label}"), Style::default().fg(MUTED)),
    ];
    if let Some(code) = code {
        spans.push(Span::styled(
            format!(" · {code}"),
            Style::default().fg(MUTED),
        ));
    }
    Line::from(spans)
}

fn format_fixed(value: Fixed) -> String {
    let micros = i128::from(value.micros());
    let negative = micros.is_negative();
    let absolute = micros.abs();
    let whole = absolute / i128::from(Fixed::SCALE);
    let fraction = absolute % i128::from(Fixed::SCALE);
    let sign = if negative { "-" } else { "" };
    if fraction == 0 {
        format!("{sign}{whole}")
    } else {
        let fraction = format!("{fraction:06}").trim_end_matches('0').to_owned();
        format!("{sign}{whole}.{fraction}")
    }
}

fn resource_bar(current: Fixed, maximum: Fixed, width: usize) -> String {
    let maximum = i128::from(maximum.micros());
    let filled = if maximum <= 0 {
        0
    } else {
        let current = i128::from(current.micros()).clamp(0, maximum);
        usize::try_from(current * width as i128 / maximum).unwrap_or(width)
    };
    format!(
        "{}{}",
        "━".repeat(filled),
        "─".repeat(width.saturating_sub(filled))
    )
}

fn format_world_time(time: WorldTime) -> String {
    let ticks = time.ticks();
    let day = ticks / 86_400 + 1;
    let hour = ticks % 86_400 / 3_600;
    let minute = ticks % 3_600 / 60;
    let second = ticks % 60;
    if ticks >= 86_400 {
        format!("Day {day} · {hour:02}:{minute:02}")
    } else {
        format!("{hour:02}:{minute:02}:{second:02}")
    }
}

fn format_parameter(value: &ParameterValue) -> String {
    match value {
        ParameterValue::Bool(value) => if *value { "yes" } else { "no" }.to_owned(),
        ParameterValue::Fixed(value) => format_fixed(*value),
        ParameterValue::Counter(value) => value.to_string(),
        ParameterValue::Enum(value) => value.as_str().to_owned(),
        ParameterValue::TagSet(values) => format!("{} tags", values.len()),
        ParameterValue::ObjectRef(value) => value.to_string(),
    }
}

const fn life_label(state: LifeState) -> &'static str {
    match state {
        LifeState::Alive => "Alive",
        LifeState::Downed => "Downed",
        LifeState::Dead => "Dead",
    }
}

const fn action_label(state: ActionState) -> &'static str {
    match state {
        ActionState::Idle => "Idle",
        ActionState::Acting { .. } => "Acting",
        ActionState::Waiting => "Waiting",
    }
}

const fn posture_label(state: Posture) -> &'static str {
    match state {
        Posture::Standing => "Standing",
        Posture::Sitting => "Sitting",
        Posture::Prone => "Prone",
    }
}

const fn phase_label(phase: RuntimePhase) -> &'static str {
    match phase {
        RuntimePhase::Idle | RuntimePhase::Completed => "Ready",
        RuntimePhase::PersistingInput => "Saving your words",
        RuntimePhase::NarratorThinking => "Narrator is thinking",
        RuntimePhase::ResolvingOrchestration => "Resolving the scene",
        RuntimePhase::NpcThinking => "NPC is responding",
        RuntimePhase::UpdatingWorld => "Updating the world",
        RuntimePhase::Cancelled => "Cancelled",
        RuntimePhase::Failed => "Turn failed",
    }
}

const fn header_phase_label(phase: RuntimePhase) -> &'static str {
    match phase {
        RuntimePhase::Idle | RuntimePhase::Completed => "ready",
        RuntimePhase::PersistingInput
        | RuntimePhase::NarratorThinking
        | RuntimePhase::ResolvingOrchestration
        | RuntimePhase::NpcThinking
        | RuntimePhase::UpdatingWorld => "working",
        RuntimePhase::Cancelled => "cancelled",
        RuntimePhase::Failed => "failed",
    }
}

const fn spinner(frame: u8) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(frame as usize) % FRAMES.len()]
}
