use crate::{
    app::{App, Focus, Level, Pending, composer_display_width, composer_rows},
    config::AgentMode,
    document::{display_width, grapheme_width},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.review_buttons.clear();
    app.review_row_hunks.clear();
    let area = frame.area();
    let theme = app.theme;
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.fg)),
        area,
    );
    if area.width < 30 || area.height < 10 {
        app.editor_area = Rect::default();
        app.chat_area = Rect::default();
        frame.render_widget(
            Paragraph::new("Terminal too small (minimum 30 × 10).\nResize, or Ctrl+Q to exit.")
                .wrap(ratatui::widgets::Wrap { trim: false }),
            area,
        );
        if app.save_prompt.is_some() {
            save_prompt(frame, app);
        } else if app.pending.is_some() {
            confirmation(frame, app);
        } else if app.live.permission.is_some() {
            permission(frame, app);
        } else if app.connect.is_some() {
            connect(frame, app);
        } else if app.model_picker.is_some() {
            model_picker(frame, app);
        } else if app.agent_picker.is_some() {
            agent_picker(frame, app);
        }
        return;
    }
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(area);
    let wide = rows[0].width >= 124;
    let panels = Layout::default()
        .direction(if wide {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints(if wide {
            vec![
                Constraint::Length(82 + (rows[0].width - 124) * 2 / 3),
                Constraint::Min(42),
            ]
        } else {
            vec![Constraint::Percentage(58), Constraint::Percentage(42)]
        })
        .split(rows[0]);
    app.chat_area = panels[1];
    let name = app
        .document
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "[Untitled]".into());
    let editor_block = pane(
        &format!(
            " {}{} ",
            safe(&name),
            if app.document.dirty() { " *" } else { "" }
        ),
        app.focus == Focus::Editor,
        app,
    );
    app.editor_area = editor_block.inner(panels[0]);
    frame.render_widget(editor_block, panels[0]);
    editor(frame, app);
    if app.explorer.is_some() {
        explorer(frame, app);
    } else {
        chat(frame, app);
    }
    let (row, col) = app.document.line_column();
    let status = format!(
        " {}:{} · {} · {}",
        row + 1,
        col + 1,
        if app.focus == Focus::Editor {
            "EDITOR"
        } else {
            "CHAT"
        },
        safe(&app.agents[app.agent].name),
    );
    let level = match app.level {
        Level::Info => ("INFO", theme.muted),
        Level::Success => ("OK", theme.green),
        Level::Warning => ("WARN", theme.yellow),
        Level::Error => ("ERROR", theme.red),
    };
    let detail = if let Some(review) = &app.review {
        format!(
            " {} · {} pending · Change {} of {} · Alt+[ / Alt+] navigate · Ctrl+Enter Keep · Ctrl+Backspace Undo",
            if app.document.is_temporary() {
                "TEMP SAVED · Ctrl+S filename"
            } else {
                "SAVED"
            },
            review.pending(),
            review.active + 1,
            review.hunks.len()
        )
    } else if let Some(proposal) = &app.proposal {
        format!(
            " MOCK PREVIEW: Change {} of {} · Alt+[ / Alt+] navigate · Ctrl+Enter keep · Ctrl+Backspace reject",
            proposal.active_hunk + 1,
            proposal.hunks.len()
        )
    } else {
        format!(" {}: {}", level.0, safe(&app.status))
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(status),
            Line::styled(
                detail,
                Style::default().fg(if app.proposal.is_some() || app.review.is_some() {
                    theme.yellow
                } else {
                    level.1
                }),
            ),
        ]),
        rows[1],
    );
    if app.save_prompt.is_some() {
        save_prompt(frame, app);
    } else if app.pending.is_some() {
        confirmation(frame, app);
    } else if app.live.permission.is_some() {
        permission(frame, app);
    } else if app.connect.is_some() {
        connect(frame, app);
    } else if app.model_picker.is_some() {
        model_picker(frame, app);
    } else if app.agent_picker.is_some() {
        agent_picker(frame, app);
    }
}

fn pane(title: &str, focused: bool, app: &App) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(title.to_owned())
        .border_style(Style::default().fg(if focused {
            app.theme.accent
        } else {
            app.theme.muted
        }))
}

fn editor(frame: &mut Frame, app: &mut App) {
    let area = app.editor_area;
    if area.width == 0 || area.height == 0 {
        return;
    }
    if app.review.is_some() {
        review_editor(frame, app);
        return;
    }
    app.review_gutter = 0;
    app.review_view_rows.clear();
    let preview = proposal_preview(app);
    let source = preview
        .as_ref()
        .map_or(app.document.text.as_str(), |preview| preview.text.as_str());
    let (row, column) = preview.as_ref().map_or_else(
        || (app.document.line_column().0, app.document.display_column()),
        |preview| line_display_position(source, preview.active_offset),
    );
    let width = usize::from(area.width).max(1);
    let height = usize::from(area.height);
    let selection = app.document.selection();
    let mut lines: Vec<Line<'static>> = vec![];
    let mut rows = vec![];
    let mut frontmatter = false;
    let mut fence = false;
    let mut offset = 0;
    for (index, raw) in source.split('\n').enumerate() {
        let text = raw.trim_end_matches('\r');
        if text == "---" && (index == 0 || frontmatter) {
            frontmatter = !frontmatter;
        }
        if text.trim_start().starts_with("```") || text.trim_start().starts_with("~~~") {
            fence = !fence;
        }
        let heading = text.starts_with('#') && text.trim_start_matches('#').starts_with(' ');
        let mut spans = vec![];
        let mut display = 0;
        let mut segment_start = 0;
        let mut segment_width = 0;
        let mut bold = false;
        let mut italic = false;
        let mut inline_code = false;
        let graphemes = text.grapheme_indices(true).collect::<Vec<_>>();
        let mut previous_star = false;
        for (n, (byte, g)) in graphemes.iter().enumerate() {
            let w = grapheme_width(g, display);
            if segment_width > 0 && segment_width + w > width {
                lines.push(Line::from(std::mem::take(&mut spans)));
                rows.push((index, segment_start));
                segment_start = display;
                segment_width = 0;
            }
            if *g == "`" {
                inline_code = !inline_code;
            }
            if *g == "*" || *g == "_" {
                if !previous_star && graphemes.get(n + 1).is_some_and(|(_, next)| next == g) {
                    bold = !bold;
                    previous_star = true;
                } else if previous_star {
                    previous_star = false;
                } else {
                    italic = !italic;
                }
            } else {
                previous_star = false;
            }
            let mut style = Style::default().fg(app.theme.fg);
            if heading {
                style = style.fg(app.theme.accent).add_modifier(Modifier::BOLD);
            } else if frontmatter || text == "---" {
                style = style.fg(app.theme.yellow);
            } else if fence || inline_code {
                style = style.fg(app.theme.green);
            } else {
                if bold {
                    style = style.fg(app.theme.accent).add_modifier(Modifier::BOLD);
                }
                if italic {
                    style = style.fg(app.theme.yellow).add_modifier(Modifier::ITALIC);
                }
            }
            let pending_change = preview.as_ref().is_some_and(|preview| {
                preview
                    .changed
                    .iter()
                    .any(|range| range.contains(&(offset + byte)))
            });
            if pending_change {
                style = style.fg(app.theme.green).add_modifier(Modifier::BOLD);
            } else if selection
                .as_ref()
                .is_some_and(|range| range.contains(&(offset + byte)))
            {
                style = style.bg(app.theme.selection);
            }
            let rendered = if *g == "\t" { " ".repeat(w) } else { safe(g) };
            spans.push(Span::styled(rendered, style));
            display += w;
            segment_width += w;
        }
        lines.push(Line::from(spans));
        rows.push((index, segment_start));
        offset += raw.len() + 1;
    }
    let cursor_visual = rows
        .iter()
        .enumerate()
        .filter(|(_, (logical, start))| *logical == row && *start <= column)
        .map(|(visual, _)| visual)
        .next_back()
        .unwrap_or(0);
    if app.follow_editor {
        if cursor_visual < app.editor_scroll {
            app.editor_scroll = cursor_visual;
        } else if cursor_visual >= app.editor_scroll + height {
            app.editor_scroll = cursor_visual + 1 - height;
        }
    }
    app.editor_scroll = app
        .editor_scroll
        .min(lines.len().saturating_sub(height.max(1)));
    let end = (app.editor_scroll + height).min(lines.len());
    app.editor_view_rows = rows[app.editor_scroll..end].to_vec();
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(app.editor_scroll)
                .take(height)
                .collect::<Vec<_>>(),
        ),
        area,
    );
    let cursor_start = rows.get(cursor_visual).map_or(0, |(_, start)| *start);
    let cursor_column = column
        .saturating_sub(cursor_start)
        .min(width.saturating_sub(1));
    if app.focus == Focus::Editor
        && app.proposal.is_none()
        && app.pending.is_none()
        && app.live.permission.is_none()
        && cursor_visual >= app.editor_scroll
        && cursor_visual < app.editor_scroll + height
    {
        frame.set_cursor_position((
            area.x + cursor_column as u16,
            area.y + (cursor_visual - app.editor_scroll) as u16,
        ));
    }
}

struct ProposalPreview {
    text: String,
    changed: Vec<Range<usize>>,
    active_offset: usize,
}

fn proposal_preview(app: &App) -> Option<ProposalPreview> {
    let proposal = app.proposal.as_ref()?;
    let mut text = app.document.text[..proposal.range.start].to_owned();
    let mut changed = vec![];
    let mut source_offset = 0;
    let mut active_offset = text.len();
    for (index, hunk) in proposal.hunks.iter().enumerate() {
        text.push_str(&proposal.original[source_offset..hunk.range.start]);
        if index == proposal.active_hunk {
            active_offset = text.len();
        }
        if hunk.included {
            let start = text.len();
            text.push_str(&hunk.replacement);
            if start < text.len() {
                changed.push(start..text.len());
            }
        } else {
            text.push_str(&hunk.original);
        }
        source_offset = hunk.range.end;
    }
    text.push_str(&proposal.original[source_offset..]);
    text.push_str(&app.document.text[proposal.range.end..]);
    Some(ProposalPreview {
        text,
        changed,
        active_offset,
    })
}

fn line_display_position(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let row = before.bytes().filter(|byte| *byte == b'\n').count();
    let column = display_width(before.rsplit('\n').next().unwrap_or(""));
    (row, column)
}

fn diff_background(
    base: ratatui::style::Color,
    color: ratatui::style::Color,
) -> ratatui::style::Color {
    use ratatui::style::Color;
    match (base, color) {
        (Color::Rgb(r, g, b), Color::Rgb(x, y, z)) => Color::Rgb(
            ((u16::from(r) * 4 + u16::from(x)) / 5) as u8,
            ((u16::from(g) * 4 + u16::from(y)) / 5) as u8,
            ((u16::from(b) * 4 + u16::from(z)) / 5) as u8,
        ),
        _ => base,
    }
}

/// Stronger highlights for the changed words within a paired replacement line.
fn changed_words(text: &str, other: &str, added: bool) -> Vec<Range<usize>> {
    use similar::ChangeTag;
    let diff = if added {
        similar::TextDiff::from_words(other, text)
    } else {
        similar::TextDiff::from_words(text, other)
    };
    let mut offset = 0;
    let mut ranges = vec![];
    for change in diff.iter_all_changes() {
        let relevant = if added {
            change.tag() != ChangeTag::Delete
        } else {
            change.tag() != ChangeTag::Insert
        };
        if relevant {
            let end = offset + change.value().len();
            if change.tag() != ChangeTag::Equal {
                ranges.push(offset..end);
            }
            offset = end;
        }
    }
    ranges
}

fn review_editor(frame: &mut Frame, app: &mut App) {
    use similar::{ChangeTag, DiffTag, TextDiff};
    let review = app.review.as_ref().unwrap();
    let area = app.editor_area;
    // Build an old side that retains kept and manual changes; only pending edits differ.
    let mut old = app.document.text.clone();
    for hunk in review.hunks.iter().rev().filter(|h| !h.resolved) {
        old.replace_range(hunk.range.clone(), &hunk.original);
    }
    let new = &app.document.text;
    let old_lines = old.split_inclusive('\n').collect::<Vec<_>>();
    let new_lines = new.split_inclusive('\n').collect::<Vec<_>>();
    let diff = TextDiff::from_lines(&old, new);
    let digits = old_lines
        .len()
        .max(new_lines.len())
        .max(1)
        .to_string()
        .len();
    let gutter = (digits * 2 + 4).min(usize::from(area.width).saturating_sub(1));
    app.review_gutter = gutter;
    let width = usize::from(area.width).saturating_sub(gutter).max(1);
    let context = if review.expanded {
        old_lines.len().max(new_lines.len()) + 1
    } else {
        3
    };
    let mut lines = vec![];
    let mut positions = vec![];
    let mut row_hunks = vec![];
    let mut buttons = vec![];
    let mut cursor_visual = None;
    let (cursor_row, cursor_col) = (app.document.line_column().0, app.document.display_column());
    let selection = app.document.selection();
    let mut new_offsets = vec![0];
    for line in &new_lines {
        new_offsets.push(new_offsets.last().unwrap() + line.len());
    }
    let mut previous_end = 0;
    for group in diff.grouped_ops(context) {
        let Some(first) = group.first() else {
            continue;
        };
        let last = group.last().unwrap();
        if !review.expanded && first.new_range().start > previous_end {
            lines.push(Line::styled(
                "… unchanged lines · /changes expand",
                Style::default().fg(app.theme.muted),
            ));
            positions.push(None);
            row_hunks.push(None);
        }
        lines.push(Line::styled(
            format!(
                "@@ -{},{} +{},{} @@",
                first.old_range().start + 1,
                last.old_range().end - first.old_range().start,
                first.new_range().start + 1,
                last.new_range().end - first.new_range().start
            ),
            Style::default().fg(app.theme.accent),
        ));
        positions.push(None);
        row_hunks.push(None);
        previous_end = last.new_range().end;
        for op in group {
            let offset = new_offsets[op.new_range().start];
            let hunk = (op.tag() != DiffTag::Equal)
                .then(|| {
                    review.hunks.iter().enumerate().find_map(|(index, h)| {
                        (!h.resolved && h.range.start == offset).then_some(index)
                    })
                })
                .flatten();
            if let Some(index) = hunk {
                let prefix = format!(
                    "{} Change {} · ",
                    if index == review.active { ">" } else { " " },
                    index + 1
                );
                let keep_start = display_width(&prefix);
                let undo_start = keep_start + display_width("Ctrl+Enter Keep · ");
                buttons.push((
                    lines.len(),
                    index,
                    keep_start,
                    display_width("Ctrl+Enter Keep"),
                    true,
                ));
                buttons.push((
                    lines.len(),
                    index,
                    undo_start,
                    display_width("Ctrl+Backspace Undo"),
                    false,
                ));
                lines.push(Line::styled(
                    format!(
                        "{} Change {} · Ctrl+Enter Keep · Ctrl+Backspace Undo",
                        if index == review.active { ">" } else { " " },
                        index + 1
                    ),
                    Style::default().fg(if index == review.active {
                        app.theme.yellow
                    } else {
                        app.theme.muted
                    }),
                ));
                positions.push(None);
                row_hunks.push(Some(index));
            }
            for change in diff.iter_changes(&op) {
                let raw = change.value();
                let text = raw.trim_end_matches('\n').trim_end_matches('\r');
                let tag = change.tag();
                let color = match tag {
                    ChangeTag::Delete => app.theme.red,
                    ChangeTag::Insert => app.theme.green,
                    ChangeTag::Equal => app.theme.fg,
                };
                let background = if tag == ChangeTag::Equal {
                    app.theme.bg
                } else {
                    diff_background(app.theme.bg, color)
                };
                let background = if hunk == Some(review.active) {
                    diff_background(background, app.theme.yellow)
                } else {
                    background
                };
                let row_style = Style::default().fg(color).bg(background);
                let paired = if op.tag() == DiffTag::Replace {
                    if let Some(index) = change.old_index() {
                        new_lines
                            .get(op.new_range().start + index - op.old_range().start)
                            .filter(|_| {
                                op.new_range().start + index - op.old_range().start
                                    < op.new_range().end
                            })
                    } else if let Some(index) = change.new_index() {
                        old_lines
                            .get(op.old_range().start + index - op.new_range().start)
                            .filter(|_| {
                                op.old_range().start + index - op.new_range().start
                                    < op.old_range().end
                            })
                    } else {
                        None
                    }
                } else {
                    None
                };
                let highlights = paired
                    .map(|other| {
                        changed_words(
                            text,
                            other.trim_end_matches('\n').trim_end_matches('\r'),
                            tag == ChangeTag::Insert,
                        )
                    })
                    .unwrap_or_default();
                let old_number = change
                    .old_index()
                    .map(|i| (i + 1).to_string())
                    .unwrap_or_default();
                let new_number = change
                    .new_index()
                    .map(|i| (i + 1).to_string())
                    .unwrap_or_default();
                let sign = match tag {
                    ChangeTag::Delete => "−",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                let mut spans = vec![Span::styled(
                    format!("{old_number:>digits$} {new_number:>digits$} {sign} "),
                    if hunk == Some(review.active) {
                        row_style.fg(app.theme.yellow).add_modifier(Modifier::BOLD)
                    } else {
                        row_style
                    },
                )];
                let mut display = 0;
                let mut segment_start = 0;
                let mut segment_width = 0;
                let finish = |lines: &mut Vec<Line<'static>>,
                              positions: &mut Vec<Option<(usize, usize)>>,
                              spans: &mut Vec<Span<'static>>,
                              start: usize,
                              segment_width: usize| {
                    spans.push(Span::styled(
                        " ".repeat(width.saturating_sub(segment_width)),
                        row_style,
                    ));
                    lines.push(Line::from(std::mem::take(spans)));
                    positions.push(change.new_index().map(|row| (row, start)));
                };
                for (byte, g) in text.grapheme_indices(true) {
                    let w = grapheme_width(g, display);
                    if segment_width > 0 && segment_width + w > width {
                        finish(
                            &mut lines,
                            &mut positions,
                            &mut spans,
                            segment_start,
                            segment_width,
                        );
                        row_hunks.push(hunk);
                        spans.push(Span::styled(
                            format!(
                                "{}{}",
                                if hunk == Some(review.active) {
                                    ">"
                                } else {
                                    " "
                                },
                                " ".repeat(gutter.saturating_sub(1))
                            ),
                            if hunk == Some(review.active) {
                                row_style.fg(app.theme.yellow)
                            } else {
                                row_style
                            },
                        ));
                        segment_start = display;
                        segment_width = 0;
                    }
                    let mut style = row_style;
                    if highlights.iter().any(|r| r.contains(&byte)) {
                        style = style
                            .bg(diff_background(background, color))
                            .add_modifier(Modifier::BOLD);
                    }
                    if let Some(index) = change.new_index()
                        && selection
                            .as_ref()
                            .is_some_and(|r| r.contains(&(new_offsets[index] + byte)))
                    {
                        style = style.bg(app.theme.selection);
                    }
                    spans.push(Span::styled(
                        if g == "\t" { " ".repeat(w) } else { safe(g) },
                        style,
                    ));
                    display += w;
                    segment_width += w;
                }
                finish(
                    &mut lines,
                    &mut positions,
                    &mut spans,
                    segment_start,
                    segment_width,
                );
                row_hunks.push(hunk);
                if !raw.ends_with('\n') && tag != ChangeTag::Equal {
                    lines.push(Line::styled(
                        "\\ No newline at end of file",
                        Style::default().fg(app.theme.muted),
                    ));
                    positions.push(None);
                    row_hunks.push(hunk);
                }
            }
        }
    }
    if lines.is_empty() {
        // All changes manually protected: keep the live document editable.
        for (row, raw) in new.split('\n').enumerate() {
            for (segment, text) in wrapped(raw, width).into_iter().enumerate() {
                lines.push(Line::from(format!("{}{}", " ".repeat(gutter), text)));
                positions.push(Some((row, segment * width)));
                row_hunks.push(None);
            }
        }
    } else if new.ends_with('\n') || new.is_empty() {
        lines.push(Line::from(""));
        positions.push(Some((new_lines.len(), 0)));
        row_hunks.push(None);
    }
    for (visual, position) in positions.iter().enumerate() {
        if position.is_some_and(|(row, start)| row == cursor_row && start <= cursor_col) {
            cursor_visual = Some(visual);
        }
    }
    // Keyboard navigation into collapsed context expands it without changing the document.
    if app.follow_editor && cursor_visual.is_none() && !review.expanded {
        app.review.as_mut().unwrap().expanded = true;
        review_editor(frame, app);
        return;
    }
    let height = usize::from(area.height);
    if let Some(cursor) = cursor_visual
        && app.follow_editor
    {
        if cursor < app.editor_scroll {
            app.editor_scroll = cursor;
        } else if cursor >= app.editor_scroll + height {
            app.editor_scroll = cursor + 1 - height;
        }
    }
    app.editor_scroll = app.editor_scroll.min(lines.len().saturating_sub(height));
    let end = (app.editor_scroll + height).min(lines.len());
    app.review_view_rows = positions[app.editor_scroll..end].to_vec();
    app.review_row_hunks = row_hunks[app.editor_scroll..end].to_vec();
    app.review_buttons = buttons
        .into_iter()
        .filter_map(|(row, index, start, length, keep)| {
            if row < app.editor_scroll || row >= end || start >= usize::from(area.width) {
                return None;
            }
            Some((
                Rect::new(
                    area.x + start as u16,
                    area.y + (row - app.editor_scroll) as u16,
                    length.min(usize::from(area.width) - start) as u16,
                    1,
                ),
                index,
                keep,
            ))
        })
        .collect();
    app.editor_view_rows = app
        .review_view_rows
        .iter()
        .map(|p| p.unwrap_or((0, 0)))
        .collect();
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(app.editor_scroll)
                .take(height)
                .collect::<Vec<_>>(),
        ),
        area,
    );
    if app.focus == Focus::Editor
        && app.pending.is_none()
        && app.live.permission.is_none()
        && let Some(cursor) = cursor_visual
        && cursor >= app.editor_scroll
        && cursor < end
    {
        let start = positions[cursor].unwrap().1;
        frame.set_cursor_position((
            area.x
                + (gutter + cursor_col.saturating_sub(start)).min(usize::from(area.width) - 1)
                    as u16,
            area.y + (cursor - app.editor_scroll) as u16,
        ));
    }
}

fn chat(frame: &mut Frame, app: &mut App) {
    let block = pane(
        match app.agent_mode() {
            AgentMode::Chat => " Chat ",
            AgentMode::Agent => " Agent ",
        },
        app.focus == Focus::Chat,
        app,
    );
    let inside = block.inner(app.chat_area);
    frame.render_widget(block, app.chat_area);
    let composer_width = usize::from(inside.width.saturating_sub(2)).max(1);
    let composer_height = (composer_rows(&app.composer, composer_width).len() as u16 + 2)
        .clamp(3, 6)
        .min(inside.height.saturating_sub(1));
    let error_lines = if matches!(app.level, Level::Error) {
        wrapped(
            &format!("ERROR: {}", safe(&app.status)),
            usize::from(inside.width).max(1),
        )
    } else {
        vec![]
    };
    let error_height = error_lines.len().min(3) as u16;
    let error_height = error_height.min(inside.height.saturating_sub(composer_height + 2));
    let areas = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(error_height),
        Constraint::Length(1),
        Constraint::Length(composer_height),
        Constraint::Length(1),
    ])
    .split(inside);
    let width = usize::from(areas[0].width).max(1);
    let lines = chat_lines(app, width);
    let max_scroll = lines.len().saturating_sub(usize::from(areas[0].height));
    if app.follow_chat {
        app.chat_scroll = max_scroll;
    }
    app.chat_scroll = app.chat_scroll.min(max_scroll);
    if app.chat_scroll == max_scroll {
        app.follow_chat = true;
    }
    frame.render_widget(
        Paragraph::new(lines.into_iter().skip(app.chat_scroll).collect::<Vec<_>>()),
        areas[0],
    );
    frame.render_widget(
        Paragraph::new(format!("Model: {}", safe(&app.config.model)))
            .style(Style::default().fg(app.theme.muted)),
        areas[4],
    );
    frame.render_widget(
        Paragraph::new(error_lines.join("\n")).style(Style::default().fg(app.theme.red)),
        areas[1],
    );
    let preview = if let Some(label) = app.selection_label() {
        label
    } else if let Some(a) = &app.attachment {
        format!(
            "Context: {} · {} bytes{}",
            a.label,
            a.text.len(),
            if a.revision != app.document.revision {
                " · STALE"
            } else {
                ""
            }
        )
    } else {
        "Enter submits · Ctrl+J newline · Tab focus".into()
    };
    frame.render_widget(
        Paragraph::new(preview).style(Style::default().fg(app.theme.muted)),
        areas[2],
    );
    let input = Block::default()
        .borders(Borders::ALL)
        .title(" Prompt ")
        .border_style(Style::default().fg(app.theme.muted));
    let input_area = input.inner(areas[3]);
    app.composer_area = input_area;
    frame.render_widget(input, areas[3]);
    let rows = composer_rows(&app.composer, usize::from(input_area.width).max(1));
    let cursor_row = rows
        .iter()
        .rposition(|row| row.start <= app.composer_cursor)
        .unwrap_or(0);
    let cursor_col =
        composer_display_width(&app.composer[rows[cursor_row].start..app.composer_cursor]);
    let vertical = cursor_row.saturating_sub(usize::from(input_area.height).saturating_sub(1));
    let input_lines = rows
        .iter()
        .skip(vertical)
        .map(|row| {
            Line::from(
                wrapped(
                    &app.composer[row.start..row.end],
                    usize::from(input_area.width),
                )
                .into_iter()
                .next()
                .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(input_lines), input_area);
    if app.focus == Focus::Chat
        && app.proposal.is_none()
        && app.pending.is_none()
        && app.live.permission.is_none()
        && input_area.width > 0
        && input_area.height > 0
    {
        frame.set_cursor_position((
            input_area.x + cursor_col.min(usize::from(input_area.width.saturating_sub(1))) as u16,
            input_area.y + (cursor_row - vertical) as u16,
        ));
    }
    let choices = app.completions();
    if app.focus == Focus::Chat && !choices.is_empty() && areas[0].height > 0 {
        let height = (choices.len() as u16).min(6).min(areas[0].height);
        let popup = Rect::new(
            areas[0].x,
            areas[0].bottom() - height,
            areas[0].width,
            height,
        );
        let start = app.completion.saturating_sub(usize::from(height) - 1);
        app.popup_choice_area = popup;
        app.popup_choice_start = start;
        let suggestions = choices
            .iter()
            .enumerate()
            .skip(start)
            .take(usize::from(height))
            .map(|(i, c)| {
                Line::styled(
                    format!("{} {c}", if i == app.completion { ">" } else { " " }),
                    Style::default().fg(app.theme.accent).bg(app.theme.bg),
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new(suggestions), popup);
    }
}

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn chat_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![];
    for (index, message) in app.messages.iter().enumerate() {
        let name = match message.role.as_str() {
            "you" => "You",
            "assistant" | "mock" => message
                .agent
                .as_deref()
                .unwrap_or(&app.agents[app.agent].name),
            role => role,
        };
        let heading = format!("{}:", safe(name));
        lines.extend(wrapped(&heading, width).into_iter().map(|line| {
            Line::styled(
                line,
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )
        }));
        if app.active_message_index() == Some(index)
            && let Some(label) = app.activity_label()
        {
            let activity = format!(
                "{} {label}... · Esc cancels",
                SPINNER_FRAMES[app.animation_frame % SPINNER_FRAMES.len()]
            );
            lines.extend(
                wrapped(&activity, width)
                    .into_iter()
                    .map(|line| Line::styled(line, Style::default().fg(app.theme.muted))),
            );
        }
        lines.extend(wrapped(&message.text, width).into_iter().map(Line::from));
        lines.push(Line::from(""));
    }
    lines
}

fn explorer(frame: &mut Frame, app: &mut App) {
    let block = pane(
        " Files · Enter opens · Esc returns to chat ",
        app.focus == Focus::Chat,
        app,
    );
    let area = block.inner(app.chat_area);
    frame.render_widget(block, app.chat_area);
    let Some(e) = app.explorer.as_mut() else {
        return;
    };
    let height = usize::from(area.height.saturating_sub(1));
    if e.selected < e.scroll {
        e.scroll = e.selected;
    }
    if height > 0 && e.selected >= e.scroll + height {
        e.scroll = e.selected + 1 - height;
    }
    let mut lines = vec![Line::styled(
        safe(&e.path.display().to_string()),
        Style::default().fg(app.theme.muted),
    )];
    for (i, entry) in e.entries.iter().enumerate().skip(e.scroll).take(height) {
        lines.push(Line::styled(
            format!(
                "{} {}{}",
                if i == e.selected { ">" } else { " " },
                safe(&entry.name),
                if entry.directory { "/" } else { "" }
            ),
            Style::default()
                .fg(if entry.directory {
                    app.theme.accent
                } else {
                    app.theme.fg
                })
                .bg(if i == e.selected {
                    app.theme.selection
                } else {
                    app.theme.bg
                }),
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn save_prompt(frame: &mut Frame, app: &App) {
    let Some(prompt) = &app.save_prompt else {
        return;
    };
    let area = frame.area();
    let w = area.width.min(78);
    let h = area.height.min(9);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Save document ")
        .style(Style::default().bg(app.theme.bg).fg(app.theme.fg));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new("Enter a filename or path (relative to workspace)")
            .style(Style::default().fg(app.theme.accent)),
        areas[0],
    );
    let input = Block::default()
        .borders(Borders::ALL)
        .title(" Filename ")
        .border_style(Style::default().fg(app.theme.accent));
    let input_area = input.inner(areas[1]);
    frame.render_widget(input, areas[1]);
    let rows = composer_rows(&prompt.input, usize::from(input_area.width).max(1));
    let row = rows
        .iter()
        .rposition(|row| row.start <= prompt.cursor)
        .unwrap_or(0);
    frame.render_widget(
        Paragraph::new(safe(&prompt.input[rows[row].start..rows[row].end])),
        input_area,
    );
    if input_area.width > 0 && input_area.height > 0 {
        let column = composer_display_width(&prompt.input[rows[row].start..prompt.cursor]);
        frame.set_cursor_position((
            input_area.x + column.min(usize::from(input_area.width - 1)) as u16,
            input_area.y,
        ));
    }
    if let Some(error) = &prompt.error {
        frame.render_widget(
            Paragraph::new(safe(error))
                .wrap(ratatui::widgets::Wrap { trim: false })
                .style(Style::default().fg(app.theme.red)),
            areas[2],
        );
    }
    frame.render_widget(
        Paragraph::new("Enter saves · Esc cancels").style(Style::default().fg(app.theme.muted)),
        areas[3],
    );
}

fn confirmation(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let w = area.width.min(68);
    let h = area.height.min(8);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let options = [
        ("S", "Save current text and continue"),
        ("D", "Discard unsaved text; keep already-saved edits"),
        ("C / Esc", "Cancel"),
    ];
    app.popup_choice_area = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(4),
        popup.width.saturating_sub(2),
        options.len() as u16,
    );
    app.popup_choice_start = 0;
    let question = match app.pending.as_ref() {
        Some(Pending::Quit) => "Leave Queditor? Unsaved or pending work will be affected.",
        Some(Pending::Open(_)) => {
            "Open another document? Unsaved or pending work will be affected."
        }
        Some(Pending::Recover(_)) => {
            "Restore this recovery? Unsaved or pending work will be affected."
        }
        None => "Continue? Unsaved or pending work will be affected.",
    };
    let mut lines = vec![Line::styled(
        question,
        Style::default()
            .fg(app.theme.yellow)
            .add_modifier(Modifier::BOLD),
    )];
    lines.push(Line::styled(
        "↑/↓ select · Enter choose · click an answer",
        Style::default().fg(app.theme.muted),
    ));
    lines.push(Line::default());
    lines.extend(
        options.iter().enumerate().map(|(index, (key, label))| {
            popup_option(index == app.popup_selected, key, label, app)
        }),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Confirm "))
            .style(Style::default().bg(app.theme.bg)),
        popup,
    );
}

fn permission(frame: &mut Frame, app: &mut App) {
    let Some(prompt) = &app.live.permission else {
        return;
    };
    let area = frame.area();
    let w = area.width.min(90);
    let h = area.height.min(10);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    frame.render_widget(Clear, popup);
    let options = [
        ("O", "Allow once"),
        ("S", "Allow for this session"),
        ("A", "Always allow this exact path and operation"),
        ("N / Esc", "Deny"),
    ];
    app.popup_choice_area = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(5),
        popup.width.saturating_sub(2),
        options.len() as u16,
    );
    app.popup_choice_start = 0;
    let mut lines = vec![
        Line::styled(
            format!("Allow the agent to {} this path?", prompt.target.operation),
            Style::default()
                .fg(app.theme.yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            safe(&prompt.target.path.display().to_string()),
            Style::default().fg(app.theme.fg),
        ),
        Line::styled(
            if prompt.target.outside_workspace() {
                format!(
                    "WARNING: outside workspace · data goes to {}",
                    safe(&app.config.model)
                )
            } else {
                format!(
                    "Inside workspace · returned data goes to {}",
                    safe(&app.config.model)
                )
            },
            Style::default().fg(if prompt.target.outside_workspace() {
                app.theme.red
            } else {
                app.theme.muted
            }),
        ),
        Line::styled(
            "↑/↓ select · Enter choose · click an answer",
            Style::default().fg(app.theme.muted),
        ),
        Line::default(),
    ];
    lines.extend(
        options.iter().enumerate().map(|(index, (key, label))| {
            popup_option(index == app.popup_selected, key, label, app)
        }),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Agent file permission "),
            )
            .style(Style::default().bg(app.theme.bg)),
        popup,
    );
}

fn connect(frame: &mut Frame, app: &mut App) {
    use crate::app::{CONNECT_CREDENTIAL_NAMES, CONNECT_PROVIDER_NAMES, ConnectStep};
    let Some(wizard) = &app.connect else {
        return;
    };
    let area = frame.area();
    let w = area.width.min(78);
    let h = area.height.min(12);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    app.connect_choice_area = Rect::default();
    if matches!(wizard.step, ConnectStep::Provider) {
        let mut lines = vec![Line::styled(
            "Which provider do you want to connect?",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )];
        lines.push(Line::styled(
            "↑/↓ select · Enter choose · click an answer · Esc cancel",
            Style::default().fg(app.theme.muted),
        ));
        lines.push(Line::default());
        lines.extend(
            CONNECT_PROVIDER_NAMES
                .iter()
                .enumerate()
                .map(|(index, provider)| {
                    Line::styled(
                        format!(
                            "{} [{}] {}",
                            if index == wizard.provider_selected {
                                ">"
                            } else {
                                " "
                            },
                            ["O", "A", "G", "R", "C"][index],
                            provider
                        ),
                        Style::default().fg(if index == wizard.provider_selected {
                            app.theme.accent
                        } else {
                            app.theme.fg
                        }),
                    )
                }),
        );
        app.connect_choice_area = Rect::new(
            popup.x.saturating_add(1),
            popup.y.saturating_add(4),
            popup.width.saturating_sub(2),
            CONNECT_PROVIDER_NAMES.len() as u16,
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Connect provider "),
                )
                .style(Style::default().bg(app.theme.bg).fg(app.theme.fg)),
            popup,
        );
        return;
    }
    if matches!(wizard.step, ConnectStep::Credential) {
        let mut lines = vec![Line::styled(
            "Where should Queditor obtain the credential?",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )];
        lines.push(Line::styled(
            "↑/↓ select · Enter choose · click an answer · Esc cancel",
            Style::default().fg(app.theme.muted),
        ));
        lines.push(Line::default());
        lines.extend(
            CONNECT_CREDENTIAL_NAMES
                .iter()
                .enumerate()
                .map(|(index, source)| {
                    Line::styled(
                        format!(
                            "{} [{}] {}",
                            if index == wizard.credential_selected {
                                ">"
                            } else {
                                " "
                            },
                            ["K", "E", "F"][index],
                            source
                        ),
                        Style::default().fg(if index == wizard.credential_selected {
                            app.theme.accent
                        } else {
                            app.theme.fg
                        }),
                    )
                }),
        );
        lines.push(Line::default());
        lines.push(Line::styled(
            "Native local Ollama can be used without a key",
            Style::default().fg(app.theme.muted),
        ));
        app.connect_choice_area = Rect::new(
            popup.x.saturating_add(1),
            popup.y.saturating_add(4),
            popup.width.saturating_sub(2),
            CONNECT_CREDENTIAL_NAMES.len() as u16,
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Credential source "),
                )
                .style(Style::default().bg(app.theme.bg).fg(app.theme.fg)),
            popup,
        );
        return;
    }
    let (title, instructions, value, secret) = match wizard.step {
        ConnectStep::Provider => unreachable!(),
        ConnectStep::Id => (
            " Provider ID ",
            "Short ID used in models, e.g. company · Enter: next",
            wizard.input.clone(),
            false,
        ),
        ConnectStep::Name => (
            " Provider name ",
            "Display name · Enter: next",
            wizard.input.clone(),
            false,
        ),
        ConnectStep::Endpoint => (
            " Base URL ",
            "HTTPS URL including /v1 when required; loopback HTTP allowed",
            wizard.input.clone(),
            false,
        ),
        ConnectStep::Credential => unreachable!(),
        ConnectStep::Env => (
            " Environment credential ",
            "Variable name, e.g. OPENAI_API_KEY · Enter: connect",
            wizard.input.clone(),
            false,
        ),
        ConnectStep::SecretKeyring => (
            " API key → OS keyring ",
            "Type or terminal-paste key; it stays masked and out of config.yaml · Enter: connect",
            wizard.secret.clone(),
            true,
        ),
        ConnectStep::SecretFile => (
            " API key → credentials.yaml ",
            "Type or terminal-paste key; private plaintext mode 0600 on Unix · Enter: connect",
            wizard.secret.clone(),
            true,
        ),
        ConnectStep::PlaintextConfirm => (
            " Plaintext credential warning ",
            "The key will be stored unencrypted in private credentials.yaml.",
            String::new(),
            false,
        ),
    };
    let shown = if secret {
        "•".repeat(value.chars().count())
    } else {
        safe(&value)
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(instructions)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(Style::default().bg(app.theme.bg).fg(app.theme.fg)),
        popup,
    );
    if matches!(wizard.step, ConnectStep::PlaintextConfirm) {
        let options = [("Y", "Continue"), ("N", "Choose another source")];
        app.popup_choice_area = Rect::new(
            popup.x.saturating_add(1),
            popup.y.saturating_add(4),
            popup.width.saturating_sub(2),
            options.len() as u16,
        );
        app.popup_choice_start = 0;
        let lines = options
            .iter()
            .enumerate()
            .map(|(index, (key, label))| popup_option(index == app.popup_selected, key, label, app))
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Line::styled(
                "Store this API key as private plaintext?",
                Style::default()
                    .fg(app.theme.yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Rect::new(
                popup.x.saturating_add(1),
                popup.y.saturating_add(1),
                popup.width.saturating_sub(2),
                1,
            ),
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    "The key will be unencrypted in private credentials.yaml.",
                    Style::default().fg(app.theme.muted),
                ),
                Line::styled(
                    "↑/↓ select · Enter choose · click an answer",
                    Style::default().fg(app.theme.muted),
                ),
            ]),
            Rect::new(
                popup.x.saturating_add(1),
                popup.y.saturating_add(2),
                popup.width.saturating_sub(2),
                2,
            ),
        );
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(
                popup.x.saturating_add(1),
                popup.y.saturating_add(4),
                popup.width.saturating_sub(2),
                2,
            ),
        );
    }
    if matches!(
        wizard.step,
        ConnectStep::Id
            | ConnectStep::Name
            | ConnectStep::Endpoint
            | ConnectStep::Env
            | ConnectStep::SecretKeyring
            | ConnectStep::SecretFile
    ) {
        let field_width = popup.width.saturating_sub(2);
        let shown_width = display_width(&shown);
        let scroll = shown_width.saturating_sub(usize::from(field_width.saturating_sub(1)));
        let displayed = crop(&shown, scroll, usize::from(field_width));
        let field = Rect::new(
            popup.x.saturating_add(1),
            popup.bottom().saturating_sub(2),
            field_width,
            1,
        );
        frame.render_widget(
            Paragraph::new(displayed).style(Style::default().bg(app.theme.bg).fg(app.theme.fg)),
            field,
        );
        frame.set_cursor_position((
            field.x.saturating_add((shown_width - scroll) as u16),
            field.y,
        ));
    }
}

fn agent_picker(frame: &mut Frame, app: &mut App) {
    let Some(selected) = app.agent_picker else {
        return;
    };
    let area = frame.area();
    let w = area.width.min(78);
    let h = area.height.min((app.agents.len().min(15) + 4) as u16);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let visible = usize::from(h.saturating_sub(4));
    let start = selected.saturating_sub(visible.saturating_sub(1));
    app.popup_choice_area = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(2),
        popup.width.saturating_sub(2),
        app.agents.len().saturating_sub(start).min(visible) as u16,
    );
    app.popup_choice_start = start;
    let mut lines = vec![Line::styled(
        "Which agent do you want to use?",
        Style::default()
            .fg(app.theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    lines.extend(
        app.agents
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(index, agent)| {
                Line::styled(
                    format!(
                        "{} {} · {}",
                        if index == selected { ">" } else { " " },
                        safe(&agent.name),
                        safe(&agent.description)
                    ),
                    Style::default().fg(if index == selected {
                        app.theme.accent
                    } else {
                        app.theme.fg
                    }),
                )
            }),
    );
    // Keep the controls visible even when the list does not fill the popup.
    lines.resize(visible + 1, Line::from(""));
    lines.push(Line::styled(
        "↑/↓ select · Enter or click choose · Esc close",
        Style::default().fg(app.theme.muted),
    ));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Agents "))
            .style(Style::default().bg(app.theme.bg).fg(app.theme.fg)),
        popup,
    );
}

fn model_picker(frame: &mut Frame, app: &mut App) {
    let Some(picker) = &app.model_picker else {
        return;
    };
    let area = frame.area();
    let w = area.width.min(78);
    let h = area.height.min(18);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let visible = usize::from(h.saturating_sub(3));
    let start = picker.selected.saturating_sub(visible.saturating_sub(1));
    app.popup_choice_area = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(2),
        popup.width.saturating_sub(2),
        (picker.models.len().saturating_sub(start).min(visible)) as u16,
    );
    app.popup_choice_start = start;
    let mut lines = vec![Line::styled(
        "Which model do you want to use?",
        Style::default()
            .fg(app.theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    lines.extend(
        picker
            .models
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(index, model)| {
                Line::styled(
                    format!(
                        "{} {}",
                        if index == picker.selected { ">" } else { " " },
                        safe(model)
                    ),
                    Style::default().fg(if index == picker.selected {
                        app.theme.accent
                    } else {
                        app.theme.fg
                    }),
                )
            })
            .collect::<Vec<_>>(),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Models · Enter select · Esc close "),
            )
            .style(Style::default().bg(app.theme.bg)),
        popup,
    );
}

fn popup_option<'a>(selected: bool, key: &'a str, label: &'a str, app: &App) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            if selected { "> " } else { "  " },
            Style::default().fg(app.theme.accent),
        ),
        Span::styled(
            format!("[{key}] "),
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label,
            Style::default().fg(app.theme.fg).bg(if selected {
                app.theme.selection
            } else {
                app.theme.bg
            }),
        ),
    ])
}

fn safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '�'
            } else {
                c
            }
        })
        .collect()
}
fn crop(text: &str, start: usize, width: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    for g in text.graphemes(true) {
        let w = grapheme_width(g, col);
        let end = col + w;
        if end > start && col < start + width {
            if g == "\t" || col < start || end > start + width {
                out.push_str(&" ".repeat(end.min(start + width) - col.max(start)));
            } else {
                out.push_str(&safe(g));
            }
        }
        col = end;
    }
    out
}
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut result = vec![];
    for line in safe(text).split('\n') {
        let mut current = String::new();
        let mut col = 0;
        for g in line.graphemes(true) {
            let mut w = grapheme_width(g, col);
            if col + w > width && col > 0 {
                result.push(current);
                current = String::new();
                col = 0;
                w = grapheme_width(g, 0);
            }
            if g == "\t" {
                current.push_str(&" ".repeat(w.min(width)));
            } else {
                current.push_str(g);
            }
            col += w;
        }
        result.push(current);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn review_fixture(before: &str, after: &str) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("review.md");
        std::fs::write(&path, after).unwrap();
        let mut app = App::new(None).unwrap();
        app.document = crate::document::Document::open(&path).unwrap();
        app.review = Some(crate::review::Review::new(before.into(), after.into()));
        app.focus = Focus::Editor;
        (dir, app)
    }

    fn click(app: &mut App, x: u16, y: u16) {
        app.mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: x,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
    }

    #[test]
    fn cursor_navigation_highlights_and_shortcuts_resolve_the_cursor_block() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        for keep in [true, false] {
            let before = "old one\ncontext\nold two\nend\n";
            let after = "new one\ncontext\nnew two\nend\n";
            let (_dir, mut app) = review_fixture(before, after);
            let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let first_row = app
                .review_view_rows
                .iter()
                .position(|p| *p == Some((0, 0)))
                .unwrap();
            let first_bg = terminal.backend().buffer()[(
                app.editor_area.x + app.review_gutter as u16 + 4,
                app.editor_area.y + first_row as u16,
            )]
                .bg;
            app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            assert_eq!(app.review.as_ref().unwrap().active, 1);
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            assert_ne!(
                terminal.backend().buffer()[(
                    app.editor_area.x + app.review_gutter as u16 + 4,
                    app.editor_area.y + first_row as u16
                )]
                    .bg,
                first_bg
            );
            app.key(KeyEvent::new(
                if keep {
                    KeyCode::Enter
                } else {
                    KeyCode::Backspace
                },
                KeyModifiers::CONTROL,
            ));
            let review = app.review.as_ref().unwrap();
            assert!(!review.hunks[0].resolved);
            assert!(review.hunks[1].resolved);
            let expected = if keep {
                after
            } else {
                "new one\ncontext\nold two\nend\n"
            };
            assert_eq!(app.document.text, expected);
            assert_eq!(
                std::fs::read_to_string(app.document.path.as_ref().unwrap()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn clicking_labels_targets_their_block_not_the_active_block() {
        for (width, height) in [(140, 40), (80, 30), (30, 10)] {
            for keep in [true, false] {
                let (_dir, mut app) =
                    review_fixture("old\ncontext\nother\n", "new\ncontext\nchanged\n");
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                app.follow_editor = false;
                // Bring the second block's controls into view on short terminals.
                app.editor_scroll = 4;
                terminal.draw(|f| draw(f, &mut app)).unwrap();
                let button = app
                    .review_buttons
                    .iter()
                    .find(|(_, i, k)| *i == 1 && *k == keep)
                    .copied();
                if width == 30 && !keep {
                    assert!(
                        button.is_none(),
                        "clipped-away Undo must not have a hit target"
                    );
                    continue;
                }
                let (rect, _, _) = button.unwrap();
                assert!(rect.right() <= app.editor_area.right());
                assert_eq!(app.review.as_ref().unwrap().active, 0);
                click(&mut app, rect.x, rect.y);
                let review = app.review.as_ref().unwrap();
                assert!(!review.hunks[0].resolved);
                assert!(review.hunks[1].resolved);
                assert!(
                    app.review_buttons.is_empty(),
                    "old hit targets are invalidated"
                );
                assert_eq!(
                    app.document.text,
                    if keep {
                        "new\ncontext\nchanged\n"
                    } else {
                        "new\ncontext\nother\n"
                    }
                );
            }
        }
    }

    #[test]
    fn deleted_rows_and_headers_select_without_moving_the_edit_cursor() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        for header in [true, false] {
            let (_dir, mut app) =
                review_fixture("old\ncontext\ndeleted\nend\n", "new\ncontext\nend\n");
            let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let row = if header {
                let (rect, _, _) = app.review_buttons.iter().find(|(_, i, _)| *i == 1).unwrap();
                usize::from(rect.y - app.editor_area.y)
            } else {
                app.review_row_hunks
                    .iter()
                    .enumerate()
                    .filter(|(r, h)| **h == Some(1) && app.review_view_rows[*r].is_none())
                    .nth(1)
                    .unwrap()
                    .0
            };
            let cursor = app.document.cursor;
            let x = app.editor_area.x;
            let y = app.editor_area.y + row as u16;
            click(&mut app, x, y);
            assert_eq!(app.document.cursor, cursor);
            assert_eq!(app.review.as_ref().unwrap().active, 1);
            app.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
            assert_eq!(app.document.text, "new\ncontext\ndeleted\nend\n");
            assert!(!app.review.as_ref().unwrap().hunks[0].resolved);
        }
    }

    #[test]
    fn wrapped_rows_select_their_change_and_failed_mouse_undo_is_transactional() {
        let after = format!(
            "new\n{}changed {}\n",
            "context\n".repeat(20),
            "界👩‍💻\t".repeat(40)
        );
        let before = format!("old\n{}other\n", "context\n".repeat(20));
        let (_dir, mut app) = review_fixture(&before, &after);
        app.review.as_mut().unwrap().expanded = false;
        app.follow_editor = false;
        app.editor_scroll = 8;
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let row = app
            .review_view_rows
            .iter()
            .enumerate()
            .find(|(r, p)| {
                p.is_some_and(|(_, start)| start > 0) && app.review_row_hunks[*r] == Some(1)
            })
            .unwrap()
            .0;
        let x = app.editor_area.x + app.review_gutter as u16;
        let y = app.editor_area.y + row as u16;
        click(&mut app, x, y);
        assert_eq!(app.review.as_ref().unwrap().active, 1);
        assert!(app.document.text.is_char_boundary(app.document.cursor));
        app.review.as_mut().unwrap().active = 0;
        let (rect, _, _) = app
            .review_buttons
            .iter()
            .find(|(_, i, keep)| *i == 1 && !keep)
            .copied()
            .unwrap();
        let path = app.document.path.clone().unwrap();
        std::fs::write(&path, "external change").unwrap();
        click(&mut app, rect.x, rect.y);
        assert_eq!(app.document.text, after);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external change");
        assert_eq!(app.review.as_ref().unwrap().pending(), 2);
        assert!(matches!(app.level, Level::Error));
    }

    #[test]
    fn applied_review_renders_unified_lines_word_highlights_and_editable_cursor() {
        for (width, height) in [(140, 40), (80, 30), (30, 10)] {
            let mut app = App::new(None).unwrap();
            let before = "heading\nold words 界\nend\n";
            let after = "heading\nnew words 界\nend\n";
            app.document.text = after.into();
            app.document.move_to("heading\n".len(), false);
            app.review = Some(crate::review::Review::new(before.into(), after.into()));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let editor_rows = (app.editor_area.y..app.editor_area.bottom())
                .map(|y| {
                    (app.editor_area.x..app.editor_area.right())
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            let screen = editor_rows.join("\n");
            if height > 10 {
                assert!(screen.contains("@@ -"));
                assert!(screen.contains("− old words"));
                assert!(screen.contains("+ new words"));
                let row = editor_rows
                    .iter()
                    .position(|r| r.contains("+ new words"))
                    .unwrap();
                let x = app.editor_area.x + app.review_gutter as u16;
                let y = app.editor_area.y + row as u16;
                assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
                assert!(!buffer[(x + 4, y)].modifier.contains(Modifier::BOLD));
                let deleted = editor_rows
                    .iter()
                    .position(|r| r.contains("− old words"))
                    .unwrap();
                let cursor = app.document.cursor;
                app.mouse(crossterm::event::MouseEvent {
                    kind: crossterm::event::MouseEventKind::Down(
                        crossterm::event::MouseButton::Left,
                    ),
                    column: x,
                    row: app.editor_area.y + deleted as u16,
                    modifiers: crossterm::event::KeyModifiers::NONE,
                });
                assert_eq!(app.document.cursor, cursor);
                app.mouse(crossterm::event::MouseEvent {
                    kind: crossterm::event::MouseEventKind::Down(
                        crossterm::event::MouseButton::Left,
                    ),
                    column: x + 2,
                    row: y,
                    modifiers: crossterm::event::KeyModifiers::NONE,
                });
                assert_eq!(app.document.cursor, "heading\n".len() + 2);
            }
            let cursor = terminal.get_cursor_position().unwrap();
            assert!(cursor.x < width && cursor.y < height);
        }
    }

    #[test]
    fn applied_review_wraps_unicode_and_expands_collapsed_context_for_navigation() {
        let mut app = App::new(None).unwrap();
        let before = format!("{}old\n{}", "context\n".repeat(20), "context\n".repeat(20));
        let after = before.replace("old", &"界👩‍💻\t".repeat(40));
        app.document.text = after.clone();
        app.review = Some(crate::review::Review::new(before, after.clone()));
        app.document.move_to("context\n".repeat(20).len(), false);
        app.command("/changes collapse").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(!app.review.as_ref().unwrap().expanded);
        assert_eq!(app.document.text, after);
        app.document.move_to(0, false);
        app.follow_editor = true;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(app.review.as_ref().unwrap().expanded);
        assert!(
            app.review_view_rows
                .iter()
                .any(|p| matches!(p, Some((0, 0))))
        );
    }
    #[test]
    fn messages_keep_agent_names_and_legacy_roles_render_readably() {
        let mut app = App::new(None).unwrap();
        app.messages.clear();
        app.agent = 0;
        app.message("you", "Hello");
        app.message("assistant", "Plan");
        app.agent = 1;
        app.message("mock", "Offline response");
        let lines = chat_lines(&app, 80)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(lines[0], "You:");
        assert_eq!(lines[3], "Planner:");
        assert_eq!(lines[6], "Blogger:");
        let restored: crate::app::Message =
            serde_json::from_str(r#"{"role":"assistant","text":"Old response"}"#).unwrap();
        assert!(restored.agent.is_none());
        app.messages.push(restored);
        assert_eq!(chat_lines(&app, 80)[9].to_string(), "Blogger:");
        let restored: crate::app::Message =
            serde_json::from_str(&serde_json::to_string(&app.messages[1]).unwrap()).unwrap();
        assert_eq!(restored.agent.as_deref(), Some("Planner"));
    }

    #[test]
    fn current_model_is_below_prompt_not_in_global_status() {
        for (width, height) in [(140, 40), (80, 30), (30, 10)] {
            let mut app = App::new(None).unwrap();
            app.config.model = "ollama::test".into();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let row = |y, start, end| {
                (start..end)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            };
            assert!(
                row(
                    app.chat_area.bottom() - 2,
                    app.chat_area.x + 1,
                    app.chat_area.right() - 1
                )
                .contains("Model: ollama::test")
            );
            if app.composer_area.height > 0 {
                assert!(app.composer_area.bottom() <= app.chat_area.bottom() - 3);
            }
            let status = row(height - 2, 0, width);
            assert!(!status.contains("ollama::test"));
            assert!(!status.contains(&app.config.theme));
        }
    }

    #[test]
    fn agent_picker_renders_scrolled_choices_and_mouse_selects_visible_agent() {
        for (width, height) in [(140, 40), (80, 30), (30, 10)] {
            let mut app = App::new(None).unwrap();
            for index in 0..20 {
                app.agents.push(crate::app::Agent {
                    name: format!("Custom {index}"),
                    description: "Description".into(),
                    instructions: String::new(),
                });
            }
            let selected = app.agents.len() - 1;
            app.agent_picker = Some(selected);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let screen = buffer
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(screen.contains("Agents"));
            assert!(screen.contains("> Custom 19"));
            assert!(app.popup_choice_start > 0);
            assert!(app.popup_choice_area.bottom() <= height);
            let expected = app.popup_choice_start;
            app.mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: app.popup_choice_area.x,
                row: app.popup_choice_area.y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            });
            assert_eq!(app.agent, expected);
            assert!(app.agent_picker.is_none());
        }
    }

    #[test]
    fn headers_show_filename_once_and_only_assistant_mode() {
        for (width, height) in [(140, 40), (80, 30), (30, 10)] {
            for (mode, title) in [(AgentMode::Chat, "Chat"), (AgentMode::Agent, "Agent")] {
                let mut app = App::new(None).unwrap();
                app.document.path = Some("draft.md".into());
                app.document.insert("unsaved");
                app.config
                    .agent_modes
                    .insert(app.agents[app.agent].name.clone(), mode);
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| draw(frame, &mut app)).unwrap();
                let buffer = terminal.backend().buffer();
                let row = |y, start, end| {
                    (start..end)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                };
                assert!(row(0, 0, width).contains("draft.md *"));
                let header = row(app.chat_area.y, app.chat_area.x, app.chat_area.right());
                assert_eq!(header.trim_matches(['─', '┐', '┌', ' ']), title);
                assert!(!row(height - 2, 0, width).contains("draft.md"));
            }
        }
    }

    #[test]
    fn command_errors_render_above_prompt_hints() {
        for (width, height) in [(140, 40), (80, 30)] {
            let mut app = App::new(None).unwrap();
            app.focus = Focus::Chat;
            app.compose_insert("/theme unknown");
            app.key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let hint_y = app.composer_area.y - 2;
            let error_y = hint_y
                - wrapped(
                    &format!("ERROR: {}", app.status),
                    usize::from(app.chat_area.width - 2),
                )
                .len() as u16;
            let row = |y| {
                (app.chat_area.x + 1..app.chat_area.right() - 1)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            };
            assert!(row(error_y).contains("ERROR: Unknown theme"));
            assert!(row(hint_y).contains("Enter submits"));
            assert_eq!(buffer[(app.chat_area.x + 1, error_y)].fg, app.theme.red);
            assert_eq!(app.composer.trim(), "/theme unknown");

            app.notify(Level::Success, "Saved");
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let chat = (app.chat_area.y..app.chat_area.bottom())
                .flat_map(|y| {
                    (app.chat_area.x..app.chat_area.right()).map(move |x| buffer[(x, y)].symbol())
                })
                .collect::<String>();
            assert!(!chat.contains("ERROR:"));
        }
    }

    #[test]
    fn filename_popup_renders_input_cursor_and_errors_at_supported_sizes() {
        for (width, height) in [(140, 40), (80, 30), (30, 10)] {
            let mut app = App::new(None).unwrap();
            app.command("/save").unwrap();
            app.paste(&"界👩‍💻".repeat(100));
            app.save_prompt.as_mut().unwrap().error = Some("Choose another filename".into());
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let screen = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(screen.contains("Save document"));
            assert!(screen.contains("Filename"));
            assert!(screen.contains("Choose another filename"));
            let cursor = terminal.get_cursor_position().unwrap();
            assert!(cursor.x < width && cursor.y < height);
        }
    }

    #[test]
    fn long_errors_wrap_safely_at_all_supported_sizes() {
        for (width, height) in [(140, 40), (80, 30), (30, 10)] {
            let mut app = App::new(None).unwrap();
            app.focus = Focus::Chat;
            app.notify(Level::Error, "Cannot open 界👩‍💻 document. ".repeat(20));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            assert!(app.composer_area.bottom() <= app.chat_area.bottom());
        }
    }

    #[test]
    fn renders_wide_and_narrow_layouts() {
        for (width, height) in [(140, 40), (124, 30), (80, 30), (30, 10), (10, 5)] {
            let mut app = App::new(None).unwrap();
            app.document.insert("# Heading\nHello **world** 👩‍💻");
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            if width == 140 {
                assert!(app.chat_area.x > app.editor_area.x);
            }
            if width == 124 {
                assert_eq!(app.editor_area.width, 80);
                assert_eq!(app.chat_area.width - 2, 40);
            }
            if width == 80 {
                assert!(app.chat_area.y > app.editor_area.y);
            }
        }
    }
    #[test]
    fn long_unicode_composer_stays_in_bounds() {
        let mut app = App::new(None).unwrap();
        app.focus = Focus::Chat;
        app.compose_insert(&"界".repeat(100));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
    }
    #[test]
    fn long_composer_lines_soft_wrap_without_changing_input() {
        let mut app = App::new(None).unwrap();
        app.focus = Focus::Chat;
        let input = "prompt text that must wrap across several visual rows ".repeat(3);
        app.compose_insert(&input);
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        assert_eq!(app.composer, input);
        assert!(composer_rows(&input, usize::from(app.composer_area.width)).len() > 1);
    }
    #[test]
    fn long_editor_lines_soft_wrap_without_changing_text() {
        let mut app = App::new(None).unwrap();
        let text = format!("{}界{}", "a".repeat(70), "b".repeat(70));
        app.document.insert(&text);
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        assert_eq!(app.document.text, text);
        assert!(
            app.editor_view_rows
                .iter()
                .filter(|(line, _)| *line == 0)
                .count()
                > 1
        );
        assert!(
            app.editor_view_rows
                .windows(2)
                .all(|rows| rows[0].0 != rows[1].0 || rows[0].1 < rows[1].1)
        );
    }
    #[test]
    fn renders_review_and_confirmation() {
        let mut app = App::new(None).unwrap();
        app.document.insert("hello\nworld");
        app.command("/rewrite \"HELLO\\nworld\"").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("HELLO"));
        assert!(screen.contains("Ctrl+Enter keep"));
        assert!(!screen.contains("-hello"));
        app.command("/quit").unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
    }
}
