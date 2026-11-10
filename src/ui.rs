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
use std::ops::Range;
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
        } else if app.live.permission.is_some() {
            permission(frame, app);
        } else if app.connect.is_some() {
            connect(frame, app);
        } else if app.model_picker.is_some() {
            model_picker(frame, app);
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
        " {} · {}:{} · {} · {} / {}{} · {}",
        safe(&name),
        row + 1,
        col + 1,
        if app.focus == Focus::Editor {
            "EDITOR"
        } else {
            "CHAT"
        },
        safe(&app.agents[app.agent].name),
        safe(&app.config.model),
        if app.busy() { " [running]" } else { "" },
        app.config.theme
    );
    let level = match app.level {
        Level::Info => ("INFO", theme.muted),
        Level::Success => ("OK", theme.green),
        Level::Warning => ("WARN", theme.yellow),
        Level::Error => ("ERROR", theme.red),
    };
    let detail = if let Some(proposal) = &app.proposal {
        format!(
            " REVIEW: Agent change {} of {} · Alt+[ / Alt+] navigate · Ctrl+Enter keep · Ctrl+Backspace reject",
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
                Style::default().fg(if app.proposal.is_some() {
                    theme.yellow
                } else {
                    level.1
                }),
            ),
        ]),
        rows[1],
    );
    if app.pending.is_some() {
        confirmation(frame, app);
    } else if app.live.permission.is_some() {
        permission(frame, app);
    } else if app.connect.is_some() {
        connect(frame, app);
    } else if app.model_picker.is_some() {
        model_picker(frame, app);
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

fn chat(frame: &mut Frame, app: &mut App) {
    let activity = app
        .activity_label()
        .map(|label| {
            const FRAMES: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];
            format!(
                " · {label} {} · Esc cancels",
                FRAMES[app.animation_frame % FRAMES.len()]
            )
        })
        .unwrap_or_default();
    let block = pane(
        &format!(
            " Chat · {} · {} · {}{} ",
            safe(&app.agents[app.agent].name),
            app.agent_mode().label(),
            app.backend_label(),
            activity
        ),
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
        && app.live.permission.is_none()
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
    frame.render_widget(Paragraph::new("Unsaved content, pending proposal, or running agent.\nS: save accepted content and continue (agent cancelled)\nD: discard and continue · Esc: cancel")
        .wrap(ratatui::widgets::Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" Confirm "))
        .style(Style::default().bg(app.theme.bg).fg(app.theme.yellow)), popup);
}

fn permission(frame: &mut Frame, app: &App) {
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
    let text = format!(
        "Operation: {}\nPath: {}\n{}\nApproving sends the returned text/listing to {}.\nO: once · S: session · A: always for this exact path/operation\nN/Esc: deny · Ctrl+C: cancel agent · Ctrl+Q: exit",
        prompt.target.operation,
        safe(&prompt.target.path.display().to_string()),
        if prompt.target.outside_workspace() {
            "WARNING: outside current workspace"
        } else {
            "Inside current workspace (still requires approval)"
        },
        safe(&app.config.model)
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Agent file permission "),
            )
            .style(Style::default().bg(app.theme.bg).fg(app.theme.yellow)),
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
        let mut lines = vec![Line::from(
            "↑/↓ select · Enter choose · click a provider · Esc cancel",
        )];
        lines.push(Line::default());
        lines.extend(
            CONNECT_PROVIDER_NAMES
                .iter()
                .enumerate()
                .map(|(index, provider)| {
                    Line::styled(
                        format!(
                            "{} {}",
                            if index == wizard.provider_selected {
                                ">"
                            } else {
                                " "
                            },
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
            popup.y.saturating_add(3),
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
        let mut lines = vec![Line::from(
            "↑/↓ select · Enter choose · click a source · Esc cancel",
        )];
        lines.push(Line::default());
        lines.extend(
            CONNECT_CREDENTIAL_NAMES
                .iter()
                .enumerate()
                .map(|(index, source)| {
                    Line::styled(
                        format!(
                            "{} {}",
                            if index == wizard.credential_selected {
                                ">"
                            } else {
                                " "
                            },
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
            popup.y.saturating_add(3),
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
            "The key will be stored unencrypted in private credentials.yaml.\nY: continue · N: choose another source",
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

fn model_picker(frame: &mut Frame, app: &App) {
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
    let visible = usize::from(h.saturating_sub(2));
    let start = picker.selected.saturating_sub(visible.saturating_sub(1));
    let lines = picker
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
        .collect::<Vec<_>>();
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
