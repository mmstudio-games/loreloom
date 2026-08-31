use loreloom_core::{
    NoticeKind, RuntimePhase, ToolActivityState, TranscriptSpeaker, TranscriptState, UiSnapshot,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{NarrowPage, TuiApp};

pub const WIDE_LAYOUT_MINIMUM: u16 = 80;

pub fn render_ui(frame: &mut Frame<'_>, app: &TuiApp) {
    render_ui_with_state_width(frame, app, 30);
}

pub(crate) fn render_ui_with_state_width(
    frame: &mut Frame<'_>,
    app: &TuiApp,
    state_width_percent: u16,
) {
    let area = frame.area();
    let footer_height = u16::from(area.height > 0);
    let input_height = area.height.saturating_sub(footer_height).min(4);
    let content_height = area
        .height
        .saturating_sub(input_height)
        .saturating_sub(footer_height);
    let content = Rect::new(area.x, area.y, area.width, content_height);
    let input = Rect::new(
        area.x,
        area.y.saturating_add(content_height),
        area.width,
        input_height,
    );
    let footer = Rect::new(
        area.x,
        input.y.saturating_add(input.height),
        area.width,
        footer_height,
    );

    if area.width >= WIDE_LAYOUT_MINIMUM {
        let state_width = area.width.saturating_mul(state_width_percent) / 100;
        render_state(
            frame,
            &app.snapshot,
            Rect::new(content.x, content.y, state_width, content.height),
        );
        render_story(
            frame,
            app,
            Rect::new(
                content.x.saturating_add(state_width),
                content.y,
                content.width.saturating_sub(state_width),
                content.height,
            ),
        );
    } else {
        let tab_height = content.height.min(3);
        let selected = match app.narrow_page {
            NarrowPage::State => "[State] | Story   Tab: switch",
            NarrowPage::Story => "State | [Story]   Tab: switch",
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
        match app.narrow_page {
            NarrowPage::State => render_state(frame, &app.snapshot, page),
            NarrowPage::Story => render_story(frame, app, page),
        }
    }

    render_input(frame, app, input);
    render_footer(frame, app, footer);
}

fn render_state(frame: &mut Frame<'_>, snapshot: &UiSnapshot, area: Rect) {
    let player = &snapshot.player;
    let mut lines = vec![
        Line::from(format!("Name: {}", player.display_name)),
        Line::from(format!("Place: {}", snapshot.scene.place_name)),
        Line::from(format!("Scene: {}", snapshot.scene.display_name)),
        Line::from(format!("Clock: {}", snapshot.scene.clock)),
        Line::from(format!(
            "State: {:?} / {:?} / {:?}",
            player.life_state, player.action_state, player.posture
        )),
    ];
    for attribute in &player.attributes {
        lines.push(Line::from(format!(
            "{}: {} (base {})",
            attribute.display_name, attribute.effective, attribute.base
        )));
    }
    for resource in &player.resources {
        lines.push(Line::from(format!(
            "{}: {}/{}",
            resource.display_name, resource.current, resource.maximum
        )));
    }
    for condition in &player.conditions {
        let display_name = condition
            .display_name
            .as_ref()
            .map_or("Unknown condition", loreloom_core::DisplayName::as_str);
        let symptom = condition
            .symptoms
            .first()
            .map_or("", loreloom_core::ShortText::as_str);
        lines.push(Line::from(format!(
            "Condition: {} {}",
            display_name, symptom
        )));
    }
    for item in &player.inventory {
        lines.push(Line::from(format!(
            "Item: {} x{}",
            item.display_name,
            item.item.stack.0.get()
        )));
    }
    for skill in &player.skills {
        lines.push(Line::from(format!(
            "Skill: {} [{}]",
            skill.display_name,
            if skill.available {
                "ready"
            } else {
                "unavailable"
            }
        )));
    }
    for goal in &player.goals {
        lines.push(Line::from(format!("Goal: {}", goal.description)));
    }
    for set in &snapshot.parameters {
        for value in &set.values {
            lines.push(Line::from(format!(
                "{}: {:?}",
                value.display_name, value.value
            )));
        }
    }
    for event in &snapshot.active_events {
        lines.push(Line::from(format!("Event: {}", event.display_name)));
        for option in &event.options {
            lines.push(Line::from(format!(
                "  {} [{}]",
                option.display_name,
                if option.enabled {
                    "available"
                } else {
                    "disabled"
                }
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" State ").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_story(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let mut lines = Vec::new();
    if app.snapshot.transcript.before_cursor.is_some() {
        lines.push(Line::from(Span::styled(
            "[older transcript available]",
            Style::default().fg(Color::DarkGray),
        )));
    }
    for item in &app.snapshot.transcript.items {
        let speaker = match &item.speaker {
            TranscriptSpeaker::Player { display_name, .. }
            | TranscriptSpeaker::Actor { display_name, .. } => display_name.as_str(),
            TranscriptSpeaker::Narrator => "Narrator",
            TranscriptSpeaker::System => "System",
        };
        let style = match item.state {
            TranscriptState::Committed => Style::default(),
            TranscriptState::Interrupted => Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        };
        lines.push(Line::from(Span::styled(
            format!("{speaker}: {}", item.text),
            style,
        )));
    }
    if let Some(phase) = app.working_phase {
        lines.push(Line::from(Span::styled(
            format!("{} {}…", spinner(app.spinner_frame), phase_label(phase)),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::ITALIC),
        )));
    }
    for tool in &app.snapshot.tool_activity {
        lines.push(Line::from(vec![
            Span::styled(
                format!("[{}]", tool_label(tool.state)),
                Style::default().fg(tool_color(tool.state)),
            ),
            Span::raw(format!(" {}", tool.name)),
        ]));
    }
    for notice in &app.snapshot.notices {
        let color = match notice.kind {
            NoticeKind::Info => Color::Cyan,
            NoticeKind::Warning => Color::Yellow,
            NoticeKind::Error => Color::Red,
        };
        lines.push(Line::from(Span::styled(
            format!("[{:?}] {}", notice.kind, notice.message),
            Style::default().fg(color),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Story ").borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((app.transcript_scroll, 0)),
        area,
    );
}

fn render_input(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let title = if app.can_submit() {
        " Input  Enter: send · Alt+Enter/Ctrl+J: newline "
    } else {
        " Input  waiting · Esc: cancel "
    };
    let style = if app.can_submit() {
        Style::default()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new(app.editor.text_with_cursor())
            .style(style)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let phase = app.effective_phase();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", phase_label(phase)),
                Style::default().fg(if app.working_phase.is_some() {
                    Color::Yellow
                } else {
                    Color::Green
                }),
            ),
            Span::raw(format!(
                "rev {} · Ctrl+C quit · PgUp/PgDn scroll",
                app.snapshot.revision
            )),
        ])),
        area,
    );
}

const fn tool_label(state: ToolActivityState) -> &'static str {
    match state {
        ToolActivityState::Pending => "pending",
        ToolActivityState::Succeeded => "succeeded",
        ToolActivityState::Rejected => "rejected",
        ToolActivityState::Failed => "failed",
    }
}

const fn tool_color(state: ToolActivityState) -> Color {
    match state {
        ToolActivityState::Pending => Color::Yellow,
        ToolActivityState::Succeeded => Color::Green,
        ToolActivityState::Rejected => Color::Magenta,
        ToolActivityState::Failed => Color::Red,
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

const fn spinner(frame: u8) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(frame as usize) % FRAMES.len()]
}
