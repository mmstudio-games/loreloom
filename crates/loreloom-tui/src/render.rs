use loreloom_core::{
    ActionState, Fixed, LifeState, ModPackageStatus, NoticeKind, ParameterValue, Posture,
    RuntimePhase, ToolActivity, ToolActivityState, TranscriptSpeaker, TranscriptState, UiSnapshot,
    WorldTime,
};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use ratatui_image::{Image, protocol::Protocol};
use unicode_segmentation::UnicodeSegmentation;

use crate::{NarrowPage, TuiApp, TuiOverlay};

pub const WIDE_LAYOUT_MINIMUM: u16 = 80;

pub(crate) const HEADER_HEIGHT: u16 = 2;
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
    render_ui_with_appearance(frame, app, state_width_percent, None, None);
}

pub(crate) fn render_ui_with_appearance(
    frame: &mut Frame<'_>,
    app: &mut TuiApp,
    state_width_percent: u16,
    portrait: Option<&Protocol>,
    portrait_status: Option<&str>,
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
        render_wide(
            frame,
            app,
            main,
            state_width_percent,
            portrait,
            portrait_status,
        );
    } else {
        render_narrow(frame, app, main);
    }
    render_footer(frame, app, footer, area.width < WIDE_LAYOUT_MINIMUM);
    if app.overlay == Some(TuiOverlay::Mods) {
        render_mods_overlay(frame, app, area);
    }
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
        Paragraph::new(Span::styled(
            header_phase_label(app.effective_phase()),
            Style::default().fg(if app.working_phase.is_some() {
                Color::Yellow
            } else {
                Color::Green
            }),
        ))
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

fn render_wide(
    frame: &mut Frame<'_>,
    app: &mut TuiApp,
    area: Rect,
    state_width_percent: u16,
    portrait: Option<&Protocol>,
    portrait_status: Option<&str>,
) {
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

    let state_area = render_portrait(frame, sidebar, portrait, portrait_status);
    render_state(frame, &app.snapshot, state_area, true);
    frame.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(MUTED)),
        sidebar,
    );
    render_story(frame, app, story);
    render_input(frame, app, composer);
}

fn render_portrait(
    frame: &mut Frame<'_>,
    area: Rect,
    portrait: Option<&Protocol>,
    status: Option<&str>,
) -> Rect {
    let Some(portrait) = portrait else {
        if let Some(status) = status
            && area.height > 1
        {
            frame.render_widget(
                Paragraph::new(status).style(Style::default().fg(MUTED)),
                Rect::new(
                    area.x.saturating_add(1),
                    area.y,
                    area.width.saturating_sub(2),
                    1,
                ),
            );
            return Rect::new(
                area.x,
                area.y.saturating_add(2),
                area.width,
                area.height.saturating_sub(2),
            );
        }
        return area;
    };
    let size = portrait.size();
    let image_width = size.width.min(area.width.saturating_sub(2));
    let image_height = size.height.min(area.height.saturating_sub(2));
    let image_area = Rect::new(
        area.x
            .saturating_add(area.width.saturating_sub(image_width) / 2),
        area.y,
        image_width,
        image_height,
    );
    frame.render_widget(Image::new(portrait).allow_clipping(true), image_area);
    let status_height = u16::from(status.is_some() && image_height < area.height);
    if let Some(status) = status
        && status_height > 0
    {
        frame.render_widget(
            Paragraph::new(status)
                .alignment(Alignment::Center)
                .style(Style::default().fg(MUTED)),
            Rect::new(area.x, area.y.saturating_add(image_height), area.width, 1),
        );
    }
    let consumed = image_height.saturating_add(status_height).saturating_add(1);
    Rect::new(
        area.x,
        area.y.saturating_add(consumed),
        area.width,
        area.height.saturating_sub(consumed),
    )
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

    let target = if separated {
        Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(2),
            area.height,
        )
    } else {
        area
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), target);
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
            let text = if matches!(item.speaker, TranscriptSpeaker::Narrator) {
                continuation.to_owned()
            } else {
                format!("  {continuation}")
            };
            lines.push(Line::from(Span::styled(text, text_style)));
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

fn render_input(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
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
    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(border)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border));
    let body = block.inner(area);
    app.input_width = body.width;
    frame.render_widget(block, area);
    let viewport = input_viewport(&app.editor, body.width, body.height);
    let lines = viewport
        .rows
        .into_iter()
        .map(|line| {
            let prefix_bytes = if line.starts_with("› ") {
                "› ".len()
            } else if line.starts_with("  ") {
                2
            } else {
                0
            };
            Line::from(vec![
                Span::styled(
                    line[..prefix_bytes].to_owned(),
                    Style::default().fg(border).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    line[prefix_bytes..].to_owned(),
                    Style::default().fg(if ready { Color::Reset } else { MUTED }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), body);
    if app.overlay.is_none()
        && let Some((x, y)) = viewport.cursor
    {
        frame.set_cursor_position((body.x + x, body.y + y));
    }
}

#[derive(Debug, Default)]
struct InputViewport {
    rows: Vec<String>,
    cursor: Option<(u16, u16)>,
}

// Wrap graphemes once for both painting and cursor tracking. Counting bytes or
// logical newlines cannot locate the cursor after full-width text wraps.
fn input_viewport(editor: &crate::InputEditor, width: u16, height: u16) -> InputViewport {
    if width == 0 || height == 0 {
        return InputViewport::default();
    }
    let layout = input_layout(editor.text(), width);
    let (cursor_column, cursor_row) = layout.positions[editor.cursor()];
    let start = cursor_row
        .saturating_add(1)
        .saturating_sub(usize::from(height));
    InputViewport {
        rows: layout
            .rows
            .into_iter()
            .skip(start)
            .take(usize::from(height))
            .collect(),
        cursor: Some((cursor_column as u16, (cursor_row - start) as u16)),
    }
}

struct InputLayout {
    rows: Vec<String>,
    positions: Vec<(usize, usize)>,
}

fn input_layout(text: &str, width: u16) -> InputLayout {
    let width = usize::from(width.max(1));
    let indent = if width >= 4 { "  " } else { "" };
    let mut rows = vec![if indent.is_empty() {
        String::new()
    } else {
        "› ".to_owned()
    }];
    let mut positions = Vec::new();
    let mut column = indent.len();
    for grapheme in text.graphemes(true).chain(std::iter::once("")) {
        let newline = matches!(grapheme, "\n" | "\r\n" | "\r");
        let cells = if newline {
            0
        } else {
            Line::from(grapheme).width()
        };
        if !newline && (column + cells > width || column >= width) {
            rows.push(indent.to_owned());
            column = indent.len();
        }
        positions.push((column.min(width - 1), rows.len() - 1));
        if newline {
            rows.push(indent.to_owned());
            column = indent.len();
        } else {
            if let Some(row) = rows.last_mut() {
                row.push_str(grapheme);
            }
            column += cells;
        }
    }
    InputLayout { rows, positions }
}

pub(crate) fn move_input_vertical(editor: &mut crate::InputEditor, width: u16, down: bool) {
    let layout = input_layout(editor.text(), width);
    let (column, row) = layout.positions[editor.cursor()];
    let target_row = if down {
        row.saturating_add(1)
    } else {
        row.saturating_sub(1)
    };
    let target = layout
        .positions
        .iter()
        .enumerate()
        .filter(|(_, (_, candidate_row))| *candidate_row == target_row)
        .min_by_key(|(_, (candidate_column, _))| candidate_column.abs_diff(column))
        .map(|(index, _)| index);
    if let Some(target) = target {
        editor.set_cursor(target);
    } else if down {
        editor.set_cursor(editor.grapheme_count());
    }
}

fn render_footer(frame: &mut Frame<'_>, app: &TuiApp, area: Rect, narrow: bool) {
    if area.height == 0 {
        return;
    }
    let help = if narrow {
        " · Tab · F2 Mods · PgUp/PgDn · ^C"
    } else if app.can_cancel() {
        " · Ctrl+O Mods · Esc cancel · PgUp/PgDn · ^C quit"
    } else {
        " · Ctrl+O Mods · Enter send · Alt+Enter newline · ^C quit"
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
            Span::styled(help, Style::default().fg(MUTED)),
        ]))
        .alignment(Alignment::Center),
        area,
    );
}

fn render_mods_overlay(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let overlay = centered_overlay(area, 76, 30);
    let block = Block::default()
        .title(Span::styled(
            " MODS · Ctrl+O / F2 / Esc close · ↑/↓ scroll ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT));
    let body = block.inner(overlay);
    let catalog = &app.snapshot.packages;
    let enabled = catalog
        .mods
        .iter()
        .filter(|package| package.status == ModPackageStatus::Enabled)
        .collect::<Vec<_>>();
    let installed = catalog
        .mods
        .iter()
        .filter(|package| package.status == ModPackageStatus::Installed)
        .collect::<Vec<_>>();
    let mut lines = vec![
        Line::from(Span::styled(
            "WORLD",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("◆ ", Style::default().fg(ACCENT)),
            Span::styled(
                catalog.world.world_id.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            format!("  v{} · main world", catalog.world.version),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("ENABLED ({})", enabled.len()),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
    ];
    if enabled.is_empty() {
        lines.push(Line::from(Span::styled(
            "No enabled extension Mods.",
            Style::default().fg(MUTED),
        )));
    } else {
        for package in enabled {
            push_mod_package(&mut lines, package, true, None);
        }
    }
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            format!("INSTALLED, NOT ENABLED ({})", installed.len()),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
    ]);
    if installed.is_empty() {
        lines.push(Line::from(Span::styled(
            "No valid inactive Mods found in mods/.",
            Style::default().fg(MUTED),
        )));
    } else {
        for package in installed {
            push_mod_package(&mut lines, package, false, None);
        }
    }
    if catalog.unavailable_installed > 0 {
        lines.extend([
            Line::from(""),
            Line::from(vec![
                Span::styled("! ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!(
                        "{} installed candidate(s) unavailable",
                        catalog.unavailable_installed
                    ),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
        ]);
    }
    let maximum = lines.len().saturating_sub(usize::from(body.height));
    app.update_mods_layout(u16::try_from(maximum).unwrap_or(u16::MAX), body.height);
    frame.render_widget(Clear, overlay);
    frame.render_widget(block, overlay);
    frame.render_widget(Paragraph::new(lines).scroll((app.mods_scroll, 0)), body);
}

pub(crate) fn push_mod_package(
    lines: &mut Vec<Line<'static>>,
    package: &loreloom_core::ModPackageView,
    enabled: bool,
    selected: Option<bool>,
) {
    let mut heading = Vec::new();
    if let Some(selected) = selected {
        heading.push(Span::styled(
            if selected { "› " } else { "  " },
            Style::default().fg(ACCENT),
        ));
    }
    heading.extend([
        Span::styled(
            if enabled { "● " } else { "○ " },
            Style::default().fg(if enabled { Color::Green } else { ACCENT }),
        ),
        Span::styled(
            package.mod_id.to_string(),
            if selected == Some(true) {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            },
        ),
    ]);
    lines.push(Line::from(heading));
    let indent = if selected.is_some() { "    " } else { "  " };
    let dependency_label = if package.dependency_count == 1 {
        "dependency"
    } else {
        "dependencies"
    };
    lines.push(Line::from(Span::styled(
        format!(
            "{indent}v{} · {} {}",
            package.version, package.dependency_count, dependency_label
        ),
        Style::default().fg(MUTED),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "{indent}{} · {} · {}",
            content_count(
                package.content.definition_count(),
                "definition",
                "definitions"
            ),
            content_count(package.content.prompt_count(), "prompt", "prompts"),
            content_count(package.content.patches, "patch", "patches"),
        ),
        Style::default().fg(MUTED),
    )));
    push_content_counts(
        lines,
        &[
            (package.content.characters, "character", "characters"),
            (package.content.scenes, "scene", "scenes"),
            (package.content.places, "place", "places"),
        ],
        indent,
    );
    push_content_counts(
        lines,
        &[
            (package.content.items, "item", "items"),
            (package.content.skills, "skill", "skills"),
            (package.content.conditions, "condition", "conditions"),
        ],
        indent,
    );
    push_content_counts(
        lines,
        &[
            (package.content.events, "event", "events"),
            (package.content.gameplay_actions, "action", "actions"),
            (package.content.rules, "rule", "rules"),
            (package.content.parameters, "parameter", "parameters"),
        ],
        indent,
    );
    push_content_counts(
        lines,
        &[(
            package.content.support_definitions,
            "support definition",
            "support definitions",
        )],
        indent,
    );
}

fn push_content_counts(
    lines: &mut Vec<Line<'static>>,
    counts: &[(u32, &'static str, &'static str)],
    indent: &str,
) {
    let labels = counts
        .iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, singular, plural)| content_count(*count, singular, plural))
        .collect::<Vec<_>>();
    if !labels.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("{indent}{}", labels.join(" · ")),
            Style::default().fg(MUTED),
        )));
    }
}

fn content_count(count: u32, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

fn centered_overlay(area: Rect, maximum_width: u16, maximum_height: u16) -> Rect {
    let width = if area.width > 4 {
        area.width.saturating_sub(4).min(maximum_width)
    } else {
        area.width
    };
    let height = if area.height > 2 {
        area.height.saturating_sub(2).min(maximum_height)
    } else {
        area.height
    };
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
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

pub(crate) fn format_fixed(value: Fixed) -> String {
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

#[cfg(test)]
mod input_tests {
    use super::input_viewport;
    use crate::InputEditor;

    #[test]
    fn newline_after_a_full_visual_row_does_not_add_a_blank_row() {
        let editor = InputEditor::with_text("一二三四\n下一行").expect("input");
        assert_eq!(
            input_viewport(&editor, 10, 3).rows,
            ["› 一二三四", "  下一行"]
        );
    }

    #[test]
    fn vertical_navigation_uses_wrapped_rows_and_down_on_last_row_moves_to_end() {
        let mut editor = InputEditor::with_text("一二三四五六七八九十").expect("input");
        editor.set_cursor(1);
        super::move_input_vertical(&mut editor, 10, true);
        assert_eq!(editor.cursor(), 5);
        super::move_input_vertical(&mut editor, 10, false);
        assert_eq!(editor.cursor(), 1);
        super::move_input_vertical(&mut editor, 10, true);
        super::move_input_vertical(&mut editor, 10, true);
        assert_eq!(editor.cursor(), 9);
        super::move_input_vertical(&mut editor, 10, true);
        assert_eq!(editor.cursor(), 10);
        super::move_input_vertical(&mut editor, 10, true);
        assert_eq!(editor.cursor(), 10);
        super::move_input_vertical(&mut editor, 12, false);
        assert_eq!(editor.cursor(), 5, "navigation uses the resized width");
    }

    #[test]
    fn vertical_navigation_handles_explicit_newlines_and_unicode_cell_columns() {
        let mut editor = InputEditor::with_text("中文abc\r\ne\u{301}👩‍👩‍👧‍👦xy\n尾").expect("input");
        editor.set_cursor(2);
        super::move_input_vertical(&mut editor, 20, true);
        assert_eq!(
            editor.cursor(),
            9,
            "same terminal column after mixed-width text"
        );
        super::move_input_vertical(&mut editor, 20, true);
        assert_eq!(
            editor.cursor(),
            editor.grapheme_count(),
            "short final row clamps to its end"
        );
        editor.set_cursor(1);
        super::move_input_vertical(&mut editor, 20, false);
        assert_eq!(editor.cursor(), 1);
    }

    #[test]
    fn cursor_does_not_shift_or_rewrap_chinese_and_combining_characters() {
        let mut editor = InputEditor::with_text("一二三四五六七八九十e\u{301}👩‍👩‍👧‍👦").expect("input");
        let expected = input_viewport(&editor, 10, 8).rows;
        editor.move_home();
        for _ in 0..editor.grapheme_count() {
            let view = input_viewport(&editor, 10, 8);
            assert_eq!(view.rows, expected);
            let (x, y) = view.cursor.expect("cursor");
            assert!(x < 10 && y < 8);
            editor.move_right();
        }
    }

    #[test]
    fn chinese_input_scrolls_at_cell_boundaries_and_follows_cursor() {
        let mut editor = InputEditor::with_text("一二三四五六七八九十").expect("input");
        let view = input_viewport(&editor, 10, 2);
        assert_eq!(view.rows, ["  五六七八", "  九十"]);
        assert_eq!(view.cursor, Some((6, 1)));
        editor.move_home();
        let view = input_viewport(&editor, 10, 2);
        assert_eq!(view.rows, ["› 一二三四", "  五六七八"]);
        assert_eq!(view.cursor, Some((2, 0)));
        editor.move_end();
        let view = input_viewport(&editor, 12, 2);
        assert_eq!(view.rows, ["  六七八九十", "  "]);
        assert_eq!(view.cursor, Some((2, 1)));
    }

    #[test]
    fn multiline_and_grapheme_clusters_keep_cursor_in_view() {
        let mut editor = InputEditor::with_text("first\r\nsecond\n末尾e\u{301}👩‍👩‍👧‍👦").expect("input");
        let view = input_viewport(&editor, 10, 2);
        assert_eq!(view.rows, ["  second", "  末尾e\u{301}👩‍👩‍👧‍👦"]);
        assert_eq!(view.cursor, Some((9, 1)));
        editor.move_up();
        assert_eq!(input_viewport(&editor, 10, 2).cursor, Some((6, 1)));
        for (width, height) in [(1, 1), (3, 2), (6, 1), (40, 4)] {
            let (x, y) = input_viewport(&editor, width, height)
                .cursor
                .expect("cursor");
            assert!(x < width && y < height);
        }
        assert!(input_viewport(&editor, 0, 2).cursor.is_none());
        assert!(input_viewport(&editor, 10, 0).cursor.is_none());
    }
}
