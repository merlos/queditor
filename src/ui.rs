use crate::{
    app::{App, Focus, Level},
    document::{display_width, grapheme_width},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use similar::{ChangeTag, TextDiff};
use unicode_segmentation::UnicodeSegmentation;

pub fn draw(frame: &mut Frame, app: &mut App) {
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
        if app.pending.is_some() {
            confirmation(frame, app);
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
    if app.proposal.is_some() {
        review(frame, app);
    } else {
        editor(frame, app);
    }
    if app.explorer.is_some() {
        explorer(frame, app);
    } else {
        chat(frame, app);
    }
    let (row, col) = app.document.line_column();
    let status = format!(
        " {} · {}:{} · {} · {}/mock · {}",
        safe(&name),
        row + 1,
        col + 1,
        if app.focus == Focus::Editor {
            "EDITOR"
        } else {
            "CHAT"
        },
        safe(&app.agents[app.agent].name),
        app.config.theme
    );
    let level = match app.level {
        Level::Info => ("INFO", theme.muted),
        Level::Success => ("OK", theme.green),
        Level::Warning => ("WARN", theme.yellow),
        Level::Error => ("ERROR", theme.red),
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(status),
            Line::styled(
                format!(" {}: {}", level.0, safe(&app.status)),
                Style::default().fg(level.1),
            ),
        ]),
        rows[1],
    );
    if app.pending.is_some() {
        confirmation(frame, app);
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
    let row = app.document.line_column().0;
    let column = app.document.display_column();
    let height = usize::from(area.height);
    if app.follow_editor {
        if row < app.editor_scroll {
            app.editor_scroll = row;
        }
        if row >= app.editor_scroll + height {
            app.editor_scroll = row + 1 - height;
        }
        if column < app.editor_horizontal {
            app.editor_horizontal = column;
        }
        if column >= app.editor_horizontal + usize::from(area.width) {
            app.editor_horizontal = column + 1 - usize::from(area.width);
        }
    }
    let line_count = app.document.text.bytes().filter(|b| *b == b'\n').count() + 1;
    app.editor_scroll = app.editor_scroll.min(line_count.saturating_sub(1));
    let selection = app.document.selection();
    let mut lines = vec![];
    let mut frontmatter = false;
    let mut fence = false;
    let mut offset = 0;
    for (index, raw) in app.document.text.split('\n').enumerate() {
        let text = raw.trim_end_matches('\r');
        if text == "---" && (index == 0 || frontmatter) {
            frontmatter = !frontmatter;
        }
        if text.trim_start().starts_with("```") || text.trim_start().starts_with("~~~") {
            fence = !fence;
        }
        if index >= app.editor_scroll && index < app.editor_scroll + height {
            let heading = text.starts_with('#') && text.trim_start_matches('#').starts_with(' ');
            let mut spans = vec![];
            let mut display = 0;
            let mut bold = false;
            let mut italic = false;
            let mut inline_code = false;
            let graphemes = text.grapheme_indices(true).collect::<Vec<_>>();
            let mut previous_star = false;
            for (n, (byte, g)) in graphemes.iter().enumerate() {
                let w = grapheme_width(g, display);
                let start = display;
                display += w;
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
                if selection
                    .as_ref()
                    .is_some_and(|r| r.contains(&(offset + byte)))
                {
                    style = style.bg(app.theme.selection);
                }
                if display <= app.editor_horizontal
                    || start >= app.editor_horizontal + usize::from(area.width)
                {
                    continue;
                }
                let left = start.max(app.editor_horizontal);
                let right = display.min(app.editor_horizontal + usize::from(area.width));
                let rendered = if *g == "\t" || left != start || right != display {
                    " ".repeat(right.saturating_sub(left))
                } else {
                    safe(g)
                };
                spans.push(Span::styled(rendered, style));
            }
            lines.push(Line::from(spans));
        }
        offset += raw.len() + 1;
        if index >= app.editor_scroll + height {
            break;
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
    if app.focus == Focus::Editor
        && app.pending.is_none()
        && row >= app.editor_scroll
        && row < app.editor_scroll + height
        && column >= app.editor_horizontal
        && column < app.editor_horizontal + usize::from(area.width)
    {
        frame.set_cursor_position((
            area.x + (column - app.editor_horizontal) as u16,
            area.y + (row - app.editor_scroll) as u16,
        ));
    }
}

fn review(frame: &mut Frame, app: &mut App) {
    let Some(p) = &app.proposal else {
        return;
    };
    let mut lines = vec![Line::styled(
        "MOCK PROPOSAL · A accept · R reject",
        Style::default().fg(app.theme.yellow),
    )];
    let diff = TextDiff::from_lines(&p.original, &p.replacement);
    for change in diff.iter_all_changes() {
        let (prefix, color) = match change.tag() {
            ChangeTag::Delete => ("-", app.theme.red),
            ChangeTag::Insert => ("+", app.theme.green),
            ChangeTag::Equal => (" ", app.theme.muted),
        };
        for line in wrapped(
            &format!(
                "{prefix}{}",
                change.to_string().trim_end_matches(['\r', '\n'])
            ),
            usize::from(app.editor_area.width),
        ) {
            lines.push(Line::styled(line, Style::default().fg(color)));
        }
    }
    app.editor_scroll = app.editor_scroll.min(
        lines
            .len()
            .saturating_sub(usize::from(app.editor_area.height)),
    );
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(app.editor_scroll)
                .collect::<Vec<_>>(),
        ),
        app.editor_area,
    );
}

fn chat(frame: &mut Frame, app: &mut App) {
    let block = pane(
        &format!(" Chat · {} · MOCK ", safe(&app.agents[app.agent].name)),
        app.focus == Focus::Chat,
        app,
    );
    let inside = block.inner(app.chat_area);
    frame.render_widget(block, app.chat_area);
    let composer_height = (app.composer.lines().count() as u16 + 2)
        .clamp(3, 6)
        .min(inside.height);
    let areas = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(composer_height),
    ])
    .split(inside);
    let width = usize::from(areas[0].width).max(1);
    let mut lines = vec![];
    for message in &app.messages {
        lines.push(Line::styled(
            format!("{}:", message.role),
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        lines.extend(wrapped(&message.text, width).into_iter().map(Line::from));
        lines.push(Line::from(""));
    }
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
    let preview = if let Some(a) = &app.attachment {
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
    } else if app.document.selection().is_some() {
        "Selection available · /attach to pin context".into()
    } else {
        "Enter submits · Ctrl+J newline · Tab focus".into()
    };
    frame.render_widget(
        Paragraph::new(preview).style(Style::default().fg(app.theme.muted)),
        areas[1],
    );
    let input = Block::default()
        .borders(Borders::ALL)
        .title(" Prompt ")
        .border_style(Style::default().fg(app.theme.muted));
    let input_area = input.inner(areas[2]);
    app.composer_area = input_area;
    frame.render_widget(input, areas[2]);
    let before = &app.composer[..app.composer_cursor];
    let cursor_row = before.bytes().filter(|b| *b == b'\n').count();
    let cursor_col = display_width(before.rsplit('\n').next().unwrap_or(""));
    let vertical = cursor_row.saturating_sub(usize::from(input_area.height).saturating_sub(1));
    let horizontal = cursor_col.saturating_sub(usize::from(input_area.width).saturating_sub(1));
    let input_lines = app
        .composer
        .split('\n')
        .skip(vertical)
        .map(|line| Line::from(crop(line, horizontal, usize::from(input_area.width))))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(input_lines), input_area);
    if app.focus == Focus::Chat
        && app.proposal.is_none()
        && app.pending.is_none()
        && input_area.width > 0
        && input_area.height > 0
    {
        frame.set_cursor_position((
            input_area.x + (cursor_col - horizontal) as u16,
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

fn confirmation(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let w = area.width.min(68);
    let h = area.height.min(6);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new("Unsaved content or a pending proposal.\nS: save accepted content and continue\nD: discard and continue · Esc: cancel")
        .wrap(ratatui::widgets::Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" Confirm "))
        .style(Style::default().bg(app.theme.bg).fg(app.theme.yellow)), popup);
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
    fn renders_review_and_confirmation() {
        let mut app = App::new(None).unwrap();
        app.document.insert("hello\nworld");
        app.command("/demo").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        app.command("/quit").unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
    }
}
