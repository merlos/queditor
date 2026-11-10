use crate::{
    config::{Bindings, Config, Theme, config_dir},
    document::{Document, absolute, next, previous},
};
use anyhow::{Context, Result, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const COMMANDS: &[(&str, &str)] = &[
    ("/new", "New mock chat session"),
    ("/models", "Show backend/model"),
    ("/quit", "Exit safely"),
    ("/exit", "Exit safely"),
    ("/compact", "Backend availability"),
    ("/cd", "Change workspace: /cd <path>"),
    ("/ls", "List files: /ls [path]"),
    ("/files", "Open file explorer"),
    ("/save", "Save: /save [path]"),
    ("/open", "Open/create: /open <path>"),
    ("/keys", "Current keybindings"),
    ("/theme", "Choose mocha or daylight"),
    ("/agents", "Select agent: /agents [name]"),
    ("/attach", "Attach selection, document, or clear"),
    ("/demo", "Mock uppercase edit proposal"),
    ("/rewrite", "Mock proposal: /rewrite <replacement text>"),
    ("/find", "Find text: /find <text>"),
    ("/replace", "Replace all: /replace <old> <new>"),
    ("/help", "Available commands"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Editor,
    Chat,
}
#[derive(Clone, Copy)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}
pub struct Message {
    pub role: &'static str,
    pub text: String,
}
#[derive(Clone)]
pub struct Attachment {
    pub revision: u64,
    pub range: std::ops::Range<usize>,
    pub text: String,
    pub label: String,
}
pub struct Proposal {
    pub revision: u64,
    pub range: std::ops::Range<usize>,
    pub original: String,
    pub replacement: String,
}
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
}
pub struct Explorer {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub scroll: usize,
}
pub enum Pending {
    Quit,
    Open(PathBuf),
}
#[derive(Clone)]
pub struct Agent {
    pub name: String,
    pub description: String,
    pub instructions: String,
}

pub struct App {
    pub document: Document,
    pub config: Config,
    pub bindings: Bindings,
    pub theme: Theme,
    pub focus: Focus,
    pub workspace: PathBuf,
    pub messages: Vec<Message>,
    pub composer: String,
    pub composer_cursor: usize,
    pub completion: usize,
    pub chat_scroll: usize,
    pub follow_chat: bool,
    pub agents: Vec<Agent>,
    pub agent: usize,
    pub attachment: Option<Attachment>,
    pub proposal: Option<Proposal>,
    pub explorer: Option<Explorer>,
    pub pending: Option<Pending>,
    pub status: String,
    pub level: Level,
    pub quit: bool,
    pub editor_scroll: usize,
    pub editor_horizontal: usize,
    pub follow_editor: bool,
    pub editor_area: Rect,
    pub chat_area: Rect,
    pub composer_area: Rect,
    pub clipboard: String,
    pub dragging: bool,
}

impl App {
    pub fn new(path: Option<&Path>) -> Result<Self> {
        let workspace = std::env::current_dir()?.canonicalize()?;
        let (mut config, warning) = match Config::load() {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(format!("Config ignored: {e:#}"))),
        };
        let (bindings, binding_warning) = match Bindings::load(&config.keybindings) {
            Ok(b) => (b, None),
            Err(e) => {
                config.keybindings = "vscode".into();
                (
                    Bindings::load("vscode")?,
                    Some(format!("Bindings ignored; using vscode: {e:#}")),
                )
            }
        };
        let document = if let Some(path) = path {
            open_or_new(path)?
        } else {
            Document::default()
        };
        let mut app = Self {
            document,
            theme: Theme::named(&config.theme),
            config,
            bindings,
            focus: Focus::Editor,
            workspace,
            messages: vec![],
            composer: String::new(),
            composer_cursor: 0,
            completion: 0,
            chat_scroll: 0,
            follow_chat: true,
            agents: vec![
                Agent {
                    name: "Planner".into(),
                    description: "Specifications and plans".into(),
                    instructions: "Clarify intent, assumptions, risks, and acceptance criteria."
                        .into(),
                },
                Agent {
                    name: "Blogger".into(),
                    description: "Blog drafts and revisions".into(),
                    instructions:
                        "Preserve voice, audience, and meaning. Flag claims needing verification."
                            .into(),
                },
            ],
            agent: 0,
            attachment: None,
            proposal: None,
            explorer: None,
            pending: None,
            status: "Tab switches panes · Ctrl+S saves · Ctrl+Q exits · /help in chat".into(),
            level: Level::Info,
            quit: false,
            editor_scroll: 0,
            editor_horizontal: 0,
            follow_editor: true,
            editor_area: Rect::default(),
            chat_area: Rect::default(),
            composer_area: Rect::default(),
            clipboard: String::new(),
            dragging: false,
        };
        app.message("system", "Mock backend — no network requests or AI inference. Use /demo or /rewrite to test safe edit review. /help lists commands.");
        if let Err(e) = app.load_agents() {
            app.notify(
                Level::Warning,
                format!("Some custom agents were not loaded: {e:#}"),
            );
        }
        if let Some(w) = warning.or(binding_warning) {
            app.notify(Level::Warning, w);
        }
        Ok(app)
    }

    pub fn notify(&mut self, level: Level, text: impl Into<String>) {
        self.level = level;
        self.status = text.into();
    }
    pub fn message(&mut self, role: &'static str, text: impl Into<String>) {
        self.messages.push(Message {
            role,
            text: text.into(),
        });
        self.follow_chat = true;
    }

    pub fn completions(&self) -> Vec<&'static str> {
        if !self.composer.starts_with('/') || self.composer.contains(char::is_whitespace) {
            return vec![];
        }
        COMMANDS
            .iter()
            .filter(|(c, _)| c.starts_with(&self.composer))
            .map(|(c, _)| *c)
            .collect()
    }

    pub fn key(&mut self, key: KeyEvent) {
        if self.pending.is_some() {
            self.confirm_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('q') {
            self.request(Pending::Quit);
            return;
        }
        if ctrl && key.code == KeyCode::Char('s') {
            self.save(None);
            return;
        }
        if self.proposal.is_some() {
            match key.code {
                KeyCode::Char('a') | KeyCode::Enter => self.accept_proposal(),
                KeyCode::Char('r') | KeyCode::Esc => {
                    self.proposal = None;
                    self.editor_scroll = 0;
                    self.follow_editor = true;
                    self.notify(Level::Info, "Proposal rejected; document unchanged");
                }
                KeyCode::Up => self.editor_scroll = self.editor_scroll.saturating_sub(1),
                KeyCode::Down => self.editor_scroll += 1,
                KeyCode::PageUp => self.editor_scroll = self.editor_scroll.saturating_sub(10),
                KeyCode::PageDown => self.editor_scroll += 10,
                _ => {}
            }
            return;
        }
        if key.code == KeyCode::Tab {
            if self.focus == Focus::Chat && !self.completions().is_empty() {
                self.complete();
            } else {
                self.focus = if self.focus == Focus::Editor {
                    Focus::Chat
                } else {
                    Focus::Editor
                };
            }
            return;
        }
        if key.code == KeyCode::BackTab {
            if self.focus == Focus::Chat {
                self.agent = (self.agent + 1) % self.agents.len();
            } else {
                self.focus = Focus::Chat;
            }
            return;
        }
        if self.explorer.is_some() && self.focus == Focus::Chat {
            self.explorer_key(key);
            return;
        }
        match self.focus {
            Focus::Editor => self.editor_key(key),
            Focus::Chat => self.chat_key(key),
        }
    }

    fn editor_key(&mut self, key: KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT)
            || self.document.anchor.is_some() && self.config.keybindings == "emacs";
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(action) = self.bindings.editor.get(&key_name(key)).cloned() {
            self.editor_action(&action, shift);
            self.follow_editor = true;
            return;
        }
        match key.code {
            KeyCode::Left if ctrl => self.document.word_left(shift),
            KeyCode::Right if ctrl => self.document.word_right(shift),
            KeyCode::Left => self.document.left(shift),
            KeyCode::Right => self.document.right(shift),
            KeyCode::Up => self.document.vertical(-1, shift),
            KeyCode::Down => self.document.vertical(1, shift),
            KeyCode::PageUp => self.document.vertical(-10, shift),
            KeyCode::PageDown => self.document.vertical(10, shift),
            KeyCode::Home => {
                let p = if ctrl {
                    0
                } else {
                    self.document.line_start(self.document.line_column().0)
                };
                self.document.move_to(p, shift);
            }
            KeyCode::End => {
                let row = self.document.line_column().0;
                let p = if ctrl {
                    self.document.text.len()
                } else {
                    self.document.line_start(row) + self.document.line(row).len()
                };
                self.document.move_to(p, shift);
            }
            KeyCode::Backspace => self.document.backspace(),
            KeyCode::Delete => self.document.delete(),
            KeyCode::Enter => self.document.insert("\n"),
            KeyCode::Esc => self.document.anchor = None,
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.document.insert(&c.to_string())
            }
            _ => {}
        }
        self.follow_editor = true;
    }

    fn editor_action(&mut self, action: &str, selecting: bool) {
        match action {
            "left" => self.document.left(selecting),
            "right" => self.document.right(selecting),
            "up" => self.document.vertical(-1, selecting),
            "down" => self.document.vertical(1, selecting),
            "home" => self.document.move_to(
                self.document.line_start(self.document.line_column().0),
                selecting,
            ),
            "end" => {
                let row = self.document.line_column().0;
                self.document.move_to(
                    self.document.line_start(row) + self.document.line(row).len(),
                    selecting,
                );
            }
            "delete" => self.document.delete(),
            "backspace" => self.document.backspace(),
            "mark" => self.document.anchor = Some(self.document.cursor),
            "select_all" => {
                self.document.anchor = Some(0);
                self.document.cursor = self.document.text.len();
            }
            "copy" | "cut" => {
                if let Some(text) = self.document.selected_text() {
                    self.clipboard = text.into();
                    self.notify(Level::Success, "Copied to Queditor's internal clipboard");
                    if action == "cut" {
                        self.document.insert("");
                    }
                }
            }
            "paste" => self.document.insert(&self.clipboard),
            "undo" => self.document.undo(),
            "redo" => self.document.redo(),
            _ => {}
        }
    }

    fn chat_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.compose_insert("\n")
            }
            KeyCode::Char('j') if ctrl => self.compose_insert("\n"),
            KeyCode::Enter => {
                let choices = self.completions();
                if !choices.is_empty() && !choices.contains(&self.composer.as_str()) {
                    self.complete();
                } else {
                    self.submit();
                }
            }
            KeyCode::Left => self.composer_cursor = previous(&self.composer, self.composer_cursor),
            KeyCode::Right => self.composer_cursor = next(&self.composer, self.composer_cursor),
            KeyCode::Home => self.composer_cursor = 0,
            KeyCode::End => self.composer_cursor = self.composer.len(),
            KeyCode::Backspace if self.composer_cursor > 0 => {
                let start = previous(&self.composer, self.composer_cursor);
                self.composer.replace_range(start..self.composer_cursor, "");
                self.composer_cursor = start;
            }
            KeyCode::Delete if self.composer_cursor < self.composer.len() => {
                self.composer.replace_range(
                    self.composer_cursor..next(&self.composer, self.composer_cursor),
                    "",
                );
            }
            KeyCode::Up if !self.completions().is_empty() => {
                self.completion = self.completion.saturating_sub(1)
            }
            KeyCode::Down if !self.completions().is_empty() => {
                self.completion = (self.completion + 1).min(self.completions().len() - 1)
            }
            KeyCode::PageUp => {
                self.follow_chat = false;
                self.chat_scroll = self.chat_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.chat_scroll += 10;
            }
            KeyCode::Esc => {
                self.composer.clear();
                self.composer_cursor = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.compose_insert(&c.to_string())
            }
            _ => {}
        }
    }

    fn complete(&mut self) {
        let choices = self.completions();
        if let Some(c) = choices
            .get(self.completion)
            .or_else(|| choices.first())
            .copied()
        {
            self.composer = format!("{c} ");
            self.composer_cursor = self.composer.len();
            self.completion = 0;
        }
    }

    pub fn compose_insert(&mut self, text: &str) {
        self.composer.insert_str(self.composer_cursor, text);
        let end = self.composer_cursor + text.len();
        self.composer_cursor = unicode_segmentation::UnicodeSegmentation::grapheme_indices(
            self.composer.as_str(),
            true,
        )
        .map(|(i, _)| i)
        .find(|i| *i >= end)
        .unwrap_or(self.composer.len());
        self.completion = 0;
    }

    pub fn paste(&mut self, text: &str) {
        if self.pending.is_some() || self.proposal.is_some() {
            return;
        }
        if self.focus == Focus::Editor {
            self.document.insert(text);
            self.follow_editor = true;
        } else if self.explorer.is_none() {
            self.compose_insert(text);
        }
    }

    pub fn submit(&mut self) {
        let input = self.composer.trim().to_owned();
        if input.is_empty() {
            return;
        }
        self.message("you", input.clone());
        if input.starts_with('/') {
            if let Err(e) = self.command(&input) {
                self.notify(Level::Error, format!("{e:#}"));
                return; // Keep failed input for correction.
            }
        } else {
            let context = self
                .attachment
                .as_ref()
                .map(|a| format!(" Attached {} ({} bytes).", a.label, a.text.len()))
                .unwrap_or_default();
            self.message("mock", format!("{} is selected.{context} Live AI is not connected in this build. /demo previews an uppercase edit; /rewrite <text> previews a replacement.", self.agents[self.agent].name));
        }
        self.composer.clear();
        self.composer_cursor = 0;
        self.completion = 0;
    }

    pub fn command(&mut self, input: &str) -> Result<()> {
        let args = shlex::split(input).context("Unclosed quote in command")?;
        let arg = args.get(1).map(String::as_str);
        match args.first().map(String::as_str).unwrap_or("") {
            "/quit" | "/exit" => self.request(Pending::Quit),
            "/save" => { let path = arg.map(|p| self.resolve(p)); self.document.save(path.as_deref())?; self.notify(Level::Success, "Saved"); },
            "/open" => {
                if let Some(p) = arg { self.request(Pending::Open(self.resolve(p))); }
                else { self.show_explorer(self.workspace.clone())?; }
            },
            "/new" => { self.messages.clear(); self.attachment = None; self.chat_scroll = 0; self.message("system", "New mock session; document remains open."); },
            "/models" => self.message("system", "Available model: mock/local. Live provider integration is planned, not implemented."),
            "/compact" => self.message("system", "Compaction requires a live backend and is not implemented. History was preserved."),
            "/cd" => {
                let path = self.resolve(arg.context("Usage: /cd <path>")?).canonicalize()?;
                if !path.is_dir() { bail!("Not a directory"); }
                self.workspace = path; self.notify(Level::Success, format!("Workspace: {}", self.workspace.display()));
            },
            "/ls" => {
                let path = arg.map(|p| self.resolve(p)).unwrap_or_else(|| self.workspace.clone());
                let e = Explorer::load(path)?;
                self.message("files", e.entries.iter().map(|e| format!("{}{}", e.name, if e.directory { "/" } else { "" })).collect::<Vec<_>>().join("\n"));
            },
            "/files" => self.show_explorer(arg.map(|p| self.resolve(p)).unwrap_or_else(|| self.workspace.clone()))?,
            "/theme" => {
                let name = arg.context("Usage: /theme mocha|daylight")?;
                if !matches!(name, "mocha" | "daylight") { bail!("Unknown theme"); }
                self.config.theme = name.into(); self.theme = Theme::named(name); self.notify(Level::Success, format!("Theme: {name} (this run)"));
            },
            "/agents" => {
                if let Some(name) = arg { self.agent = self.agents.iter().position(|a| a.name.eq_ignore_ascii_case(name)).context("Unknown agent")?; }
                self.message("system", self.agents.iter().map(|a| format!("{}: {}", a.name, a.description)).collect::<Vec<_>>().join("\n"));
            },
            "/attach" => {
                if arg == Some("clear") { self.attachment = None; }
                else {
                    let range = if arg == Some("document") { 0..self.document.text.len() }
                        else { self.document.selection().context("Select editor text first, or use /attach document")? };
                    self.attachment = Some(Attachment { revision: self.document.revision, text: self.document.text[range.clone()].into(),
                        range, label: if arg == Some("document") { "document" } else { "selection" }.into() });
                }
                self.notify(Level::Success, if self.attachment.is_some() { "Context snapshot attached" } else { "Context cleared" });
            },
            "/demo" | "/rewrite" => {
                if self.proposal.is_some() { bail!("Resolve the current proposal first"); }
                let range = if let Some(a) = &self.attachment {
                    if a.revision != self.document.revision { bail!("Attached context is stale; reattach current text"); }
                    a.range.clone()
                } else { self.document.selection().unwrap_or(0..self.document.text.len()) };
                let original = self.document.text[range.clone()].to_owned();
                let replacement = if args[0] == "/demo" { original.to_uppercase() }
                    else { args.get(1..).filter(|a| !a.is_empty()).context("Usage: /rewrite <replacement text>")?.join(" ") };
                if original == replacement { bail!("Mock proposal has no changes"); }
                self.proposal = Some(Proposal { revision: self.document.revision, range, original, replacement });
                self.editor_scroll = 0;
                self.notify(Level::Info, "Mock proposal: A/Enter accepts · R/Esc rejects · save excludes pending edits");
            },
            "/find" => {
                let needle = args.get(1..).filter(|a| !a.is_empty()).context("Usage: /find <text>")?.join(" ");
                if needle.is_empty() { bail!("Search cannot be empty"); }
                let start = self.document.cursor;
                let pos = self.document.text[start..].find(&needle).map(|i| start + i).or_else(|| self.document.text[..start].find(&needle)).context("Text not found")?;
                self.document.move_to(pos, false); self.document.move_to(pos + needle.len(), true);
                self.focus = Focus::Editor; self.follow_editor = true;
            },
            "/replace" => {
                if args.len() != 3 || args[1].is_empty() { bail!("Usage: /replace <old> <new> (quote spaces)"); }
                let result = self.document.text.replace(&args[1], &args[2]);
                self.document.replace(0..self.document.text.len(), &result); self.follow_editor = true;
                self.notify(Level::Success, "Replace-all complete; Ctrl+Z undoes it");
            },
            "/keys" => {
                let mut text = format!("Profile: {}\nGlobal: Tab focus; Ctrl+S save; Ctrl+Q quit\nChat: Enter submit; Ctrl+J newline; Shift+Tab agent; Tab complete\nEditor: arrows, Shift+arrows select, Home/End, PgUp/PgDown\nCopy/paste shortcuts use an internal clipboard; terminal paste is supported.\n", self.config.keybindings);
                for (key, action) in &self.bindings.editor { text.push_str(&format!("{key}: {action}\n")); }
                self.message("keys", text);
            },
            "/help" => self.message("help", COMMANDS.iter().map(|(c, d)| format!("{c} — {d}")).collect::<Vec<_>>().join("\n")),
            _ => bail!("Unknown command; use /help"),
        }
        Ok(())
    }

    pub fn accept_proposal(&mut self) {
        let Some(p) = self.proposal.take() else {
            return;
        };
        if p.revision != self.document.revision
            || self.document.text.get(p.range.clone()) != Some(p.original.as_str())
        {
            self.notify(
                Level::Error,
                "Proposal is stale; regenerate it. Document unchanged.",
            );
        } else {
            self.document.replace(p.range, &p.replacement);
            self.attachment = None;
            self.notify(
                Level::Success,
                "Proposal accepted (not saved); Ctrl+Z undoes it",
            );
        }
        self.editor_scroll = 0;
        self.follow_editor = true;
    }

    fn save(&mut self, path: Option<&Path>) {
        match self.document.save(path) {
            Ok(()) => self.notify(Level::Success, "Saved"),
            Err(e) => self.notify(Level::Error, format!("{e:#}")),
        }
    }
    fn request(&mut self, operation: Pending) {
        if self.document.dirty() || self.proposal.is_some() {
            self.pending = Some(operation);
        } else {
            self.perform(operation);
        }
    }
    fn confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => self.pending = None,
            KeyCode::Char('d') => {
                if let Some(p) = self.pending.take() {
                    self.perform(p);
                }
            }
            KeyCode::Char('s') => {
                if self.document.path.is_none() {
                    self.pending = None;
                    self.focus = Focus::Chat;
                    self.notify(
                        Level::Warning,
                        "Use /save <path>, then retry opening or quitting",
                    );
                } else {
                    match self.document.save(None) {
                        Ok(()) => {
                            if let Some(p) = self.pending.take() {
                                self.perform(p);
                            }
                        }
                        Err(e) => {
                            self.notify(Level::Error, format!("Save failed: {e:#}; Esc cancels"))
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn perform(&mut self, operation: Pending) {
        match operation {
            Pending::Quit => self.quit = true,
            Pending::Open(path) => match open_or_new(&path) {
                Ok(d) => {
                    self.document = d;
                    self.proposal = None;
                    self.attachment = None;
                    self.explorer = None;
                    self.editor_scroll = 0;
                    self.editor_horizontal = 0;
                    self.follow_editor = true;
                    self.focus = Focus::Editor;
                    self.notify(Level::Success, "Document opened");
                }
                Err(e) => self.notify(Level::Error, format!("Cannot open: {e:#}")),
            },
        }
    }
    fn resolve(&self, value: &str) -> PathBuf {
        let path = if let Some(tail) = value.strip_prefix("~/") {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(tail)
        } else {
            PathBuf::from(value)
        };
        if path.is_absolute() {
            path
        } else {
            self.workspace.join(path)
        }
    }
    fn show_explorer(&mut self, path: PathBuf) -> Result<()> {
        self.explorer = Some(Explorer::load(path)?);
        self.focus = Focus::Chat;
        Ok(())
    }
    fn explorer_key(&mut self, key: KeyEvent) {
        let Some(e) = self.explorer.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.explorer = None,
            KeyCode::Up => e.selected = e.selected.saturating_sub(1),
            KeyCode::Down => e.selected = (e.selected + 1).min(e.entries.len().saturating_sub(1)),
            KeyCode::Enter => self.explorer_enter(),
            _ => {}
        }
    }
    fn explorer_enter(&mut self) {
        let Some(entry) = self
            .explorer
            .as_ref()
            .and_then(|e| e.entries.get(e.selected))
        else {
            return;
        };
        let path = entry.path.clone();
        let directory = entry.directory;
        if directory {
            if let Err(e) = self.show_explorer(path) {
                self.notify(Level::Error, format!("{e:#}"));
            }
        } else {
            self.request(Pending::Open(path));
        }
    }

    pub fn mouse(&mut self, event: MouseEvent) {
        if self.pending.is_some() || self.proposal.is_some() {
            return;
        }
        let x = event.column;
        let y = event.row;
        if self.editor_area.contains((x, y).into()) {
            match event.kind {
                MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left) => {
                    self.focus = Focus::Editor;
                    let row = self.editor_scroll + usize::from(y - self.editor_area.y);
                    let col = self.editor_horizontal + usize::from(x - self.editor_area.x);
                    let p = self.document.position_at_display(row, col);
                    self.document.move_to(
                        p,
                        matches!(event.kind, MouseEventKind::Drag(_)) && self.dragging,
                    );
                    self.dragging = true;
                    self.follow_editor = false;
                }
                MouseEventKind::ScrollUp => {
                    self.editor_scroll = self.editor_scroll.saturating_sub(3);
                    self.follow_editor = false;
                }
                MouseEventKind::ScrollDown => {
                    self.editor_scroll += 3;
                    self.follow_editor = false;
                }
                _ => {}
            }
        } else if self.chat_area.contains((x, y).into()) {
            match event.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.focus = Focus::Chat;
                    if let Some(e) = self.explorer.as_mut() {
                        let index = e.scroll + usize::from(y.saturating_sub(self.chat_area.y + 1));
                        e.selected = index.min(e.entries.len().saturating_sub(1));
                    }
                }
                MouseEventKind::ScrollUp => {
                    if let Some(e) = self.explorer.as_mut() {
                        e.selected = e.selected.saturating_sub(3);
                    } else {
                        self.chat_scroll = self.chat_scroll.saturating_sub(3);
                        self.follow_chat = false;
                    }
                }
                MouseEventKind::ScrollDown => {
                    if let Some(e) = self.explorer.as_mut() {
                        e.selected = (e.selected + 3).min(e.entries.len().saturating_sub(1));
                    } else {
                        self.chat_scroll += 3;
                    }
                }
                _ => {}
            }
        }
        if matches!(event.kind, MouseEventKind::Up(_)) {
            self.dragging = false;
        }
    }

    fn load_agents(&mut self) -> Result<()> {
        #[derive(Deserialize)]
        struct Metadata {
            name: String,
            #[serde(default)]
            description: String,
        }
        let dir = config_dir().join("agents");
        if !dir.exists() {
            return Ok(());
        }
        let mut errors = vec![];
        let mut files = fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect::<Vec<_>>();
        files.sort();
        for path in files
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
        {
            let parsed = (|| -> Result<Agent> {
                let content = fs::read_to_string(&path)?.replace("\r\n", "\n");
                let rest = content
                    .strip_prefix("---\n")
                    .context("Missing YAML frontmatter")?;
                let (header, body) = rest
                    .split_once("\n---\n")
                    .context("Missing frontmatter closing marker")?;
                let m: Metadata = serde_yaml_ng::from_str(header)?;
                if m.name.trim().is_empty()
                    || self
                        .agents
                        .iter()
                        .any(|a| a.name.eq_ignore_ascii_case(&m.name))
                {
                    bail!("Empty or duplicate agent name");
                }
                Ok(Agent {
                    name: m.name,
                    description: m.description,
                    instructions: body.trim().into(),
                })
            })();
            match parsed {
                Ok(a) => self.agents.push(a),
                Err(e) => errors.push(format!("{}: {e:#}", path.display())),
            }
        }
        if !errors.is_empty() {
            bail!("{}", errors.join("; "));
        }
        Ok(())
    }
}

impl Explorer {
    pub fn load(path: PathBuf) -> Result<Self> {
        let path = path.canonicalize()?;
        let mut entries = vec![];
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            entries.push(Entry {
                directory: entry.path().is_dir(),
                name: entry.file_name().to_string_lossy().into(),
                path: entry.path(),
            });
        }
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        if let Some(parent) = path.parent() {
            entries.insert(
                0,
                Entry {
                    path: parent.into(),
                    name: "..".into(),
                    directory: true,
                },
            );
        }
        Ok(Self {
            path,
            entries,
            selected: 0,
            scroll: 0,
        })
    }
}
fn open_or_new(path: &Path) -> Result<Document> {
    if path.exists() {
        Document::open(path)
    } else {
        let mut d = Document::default();
        d.path = Some(absolute(path)?);
        Ok(d)
    }
}
fn key_name(key: KeyEvent) -> String {
    let mut s = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        s.push_str("ctrl+");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        s.push_str("alt+");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        s.push_str("shift+");
    }
    s.push_str(&match key.code {
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        _ => format!("{:?}", key.code).to_lowercase(),
    });
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        App::new(None).unwrap()
    }
    #[test]
    fn rejection_preserves_document() {
        let mut a = app();
        a.document.insert("hello");
        a.command("/demo").unwrap();
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(a.document.text, "hello");
    }
    #[test]
    fn accepted_proposal_is_undoable() {
        let mut a = app();
        a.document.insert("hello");
        a.command("/demo").unwrap();
        a.accept_proposal();
        assert_eq!(a.document.text, "HELLO");
        a.document.undo();
        assert_eq!(a.document.text, "hello");
    }
    #[test]
    fn stale_proposal_does_not_overwrite_edits() {
        let mut a = app();
        a.document.insert("hello");
        a.command("/demo").unwrap();
        a.document.insert("!");
        a.accept_proposal();
        assert_eq!(a.document.text, "hello!");
    }
    #[test]
    fn selected_context_is_a_snapshot() {
        let mut a = app();
        a.document.insert("hello");
        a.document.move_to(0, true);
        a.command("/attach").unwrap();
        a.document.insert("new");
        assert_eq!(a.attachment.as_ref().unwrap().text, "hello");
        assert!(a.command("/demo").is_err());
    }
    #[test]
    fn quoted_commands_and_replace_are_undoable() {
        let mut a = app();
        a.document.insert("hello world hello world");
        a.command("/replace 'hello world' 'new words'").unwrap();
        assert_eq!(a.document.text, "new words new words");
        a.document.undo();
        assert_eq!(a.document.text, "hello world hello world");
    }
    #[test]
    fn pasted_newlines_do_not_submit() {
        let mut a = app();
        a.focus = Focus::Chat;
        let before = a.messages.len();
        a.paste("one\ntwo");
        assert_eq!(a.composer, "one\ntwo");
        assert_eq!(a.messages.len(), before);
    }
    #[test]
    fn saving_excludes_unaccepted_proposal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("draft.md");
        let mut a = app();
        a.document.insert("hello");
        a.command("/demo").unwrap();
        a.document.save(Some(&path)).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "hello");
    }
    #[test]
    fn dirty_quit_requires_confirmation() {
        let mut a = app();
        a.document.insert("unsaved");
        a.command("/quit").unwrap();
        assert!(!a.quit);
        assert!(a.pending.is_some());
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!a.quit);
        assert!(a.pending.is_none());
    }
}
