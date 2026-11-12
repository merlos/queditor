use crate::{
    config::{AgentMode, Bindings, Config, Credential, Provider, Theme, config_dir},
    document::{Document, absolute, grapheme_width, next, previous},
};
use anyhow::{Context, Result, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use serde::Deserialize;
use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;

pub const COMMANDS: &[(&str, &str)] = &[
    ("/new", "New chat session"),
    ("/models", "List/select model: /models [provider::model]"),
    ("/connect", "Connect or manage an AI provider"),
    ("/quit", "Exit safely"),
    ("/exit", "Exit safely"),
    ("/compact", "Compact live conversation history"),
    ("/cancel", "Cancel running agent"),
    (
        "/permissions",
        "Inspect grants; /permissions clear revokes all",
    ),
    ("/sessions", "List saved sessions; /sessions delete <id>"),
    ("/resume", "Resume saved session: /resume <id>"),
    ("/recover", "Select/restore recovery; /recover delete <id>"),
    ("/cd", "Change workspace: /cd <path>"),
    ("/ls", "List files: /ls [path]"),
    ("/files", "Open file explorer"),
    ("/save", "Save: /save [path]"),
    ("/open", "Open/create: /open <path>"),
    ("/keys", "Current keybindings"),
    ("/theme", "Choose mocha or daylight"),
    ("/agents", "Select agent: /agents [name]"),
    ("/mode", "Set agent mode: /mode chat|agent"),
    (
        "/changes",
        "Review changes: next|prev|keep|undo|keep-all|undo-all|expand|collapse (reject aliases undo)",
    ),
    ("/attach", "Attach selection, document, or clear"),
    ("/demo", "Mock uppercase edit proposal"),
    ("/rewrite", "Mock proposal: /rewrite <replacement text>"),
    ("/find", "Find text: /find <text>"),
    ("/replace", "Replace all: /replace <old> <new>"),
    ("/version", "Display Queditor version"),
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
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub role: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Model selected when this response started; never infer it on restoration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::backend::RequestUsage>,
}
#[derive(Clone)]
pub struct Attachment {
    pub revision: u64,
    pub range: std::ops::Range<usize>,
    pub text: String,
    pub label: String,
}
pub struct Proposal {
    pub mock: bool,
    pub revision: u64,
    pub range: std::ops::Range<usize>,
    pub original: String,
    pub replacement: String,
    pub hunks: Vec<ProposalHunk>,
    pub active_hunk: usize,
}
pub struct ProposalHunk {
    /// Byte range relative to `Proposal::original`.
    pub range: Range<usize>,
    pub original: String,
    pub replacement: String,
    pub included: bool,
    pub resolved: bool,
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
    Recover(String),
}
#[derive(Clone)]
pub struct Agent {
    pub name: String,
    pub description: String,
    pub instructions: String,
}

const DEFAULT_AGENT_SOURCES: &[(&str, &str)] =
    include!(concat!(env!("OUT_DIR"), "/default_agents.rs"));

impl Agent {
    fn defaults() -> Result<Vec<Self>> {
        Self::parse_defaults(DEFAULT_AGENT_SOURCES)
    }

    fn parse_markdown(source: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct Metadata {
            name: String,
            #[serde(default)]
            description: String,
        }
        let content = source.replace("\r\n", "\n");
        let rest = content
            .strip_prefix("---\n")
            .context("Missing YAML frontmatter")?;
        let (header, body) = rest
            .split_once("\n---\n")
            .context("Missing frontmatter closing marker")?;
        let metadata: Metadata = serde_yaml_ng::from_str(header)?;
        Ok(Self {
            name: metadata.name,
            description: metadata.description,
            instructions: body.trim().into(),
        })
    }

    fn parse_defaults(sources: &[(&str, &str)]) -> Result<Vec<Self>> {
        if sources.is_empty() {
            bail!("At least one default agent is required");
        }
        let mut agents: Vec<Self> = Vec::new();
        for &(filename, source) in sources {
            let agent = Self::parse_markdown(source)
                .with_context(|| format!("Invalid embedded assets/agents/{filename}"))?;
            if agent.name.trim().is_empty()
                || agent.name.len() > 200
                || agent.name.chars().any(char::is_control)
                || agent.instructions.trim().is_empty()
            {
                bail!("Default agents require a valid name and nonempty instructions");
            }
            if agents
                .iter()
                .any(|other| other.name.eq_ignore_ascii_case(&agent.name))
            {
                bail!("Default agent names must be unique");
            }
            agents.push(agent);
        }
        Ok(agents)
    }
}

pub struct SavePrompt {
    pub input: String,
    pub cursor: usize,
    pub error: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct ChatRow {
    pub message: usize,
    pub row: ComposerRow,
    pub left: usize,
}

pub struct MessageSelection {
    pub message: usize,
    pub anchor: usize,
    pub cursor: usize,
}

#[derive(Clone, Copy)]
enum ChatDrag {
    Composer,
    Message(usize),
}

pub struct App {
    pub live: crate::live::Live,
    pub document: Document,
    pub config: Config,
    pub bindings: Bindings,
    pub theme: Theme,
    pub focus: Focus,
    pub workspace: PathBuf,
    pub messages: Vec<Message>,
    pub composer: String,
    pub composer_cursor: usize,
    pub composer_anchor: Option<usize>,
    pub message_selection: Option<MessageSelection>,
    pub(crate) chat_view_rows: Vec<Option<ChatRow>>,
    pub(crate) chat_history_area: Rect,
    pub(crate) composer_view_rows: Vec<ComposerRow>,
    chat_drag: Option<ChatDrag>,
    prompt_history: Vec<String>,
    prompt_history_index: Option<usize>,
    prompt_history_draft: String,
    pub completion: usize,
    pub chat_scroll: usize,
    pub follow_chat: bool,
    pub agents: Vec<Agent>,
    pub agent: usize,
    pub attachment: Option<Attachment>,
    pub proposal: Option<Proposal>,
    pub review: Option<crate::review::Review>,
    /// Review rows map only live document lines; removed lines and headers have no cursor.
    pub review_view_rows: Vec<Option<(usize, usize)>>,
    /// Pending change owning each visible review row, including removed lines.
    pub review_row_hunks: Vec<Option<usize>>,
    /// Visible, clipped Keep/Undo labels: rectangle, change index, Keep flag.
    pub review_buttons: Vec<(Rect, usize, bool)>,
    pub review_gutter: usize,
    pub explorer: Option<Explorer>,
    pub pending: Option<Pending>,
    pub save_prompt: Option<SavePrompt>,
    pub status: String,
    pub level: Level,
    pub quit: bool,
    pub editor_scroll: usize,
    pub editor_horizontal: usize,
    /// Logical line and starting display column for each visible soft-wrapped row.
    pub editor_view_rows: Vec<(usize, usize)>,
    pub follow_editor: bool,
    pub editor_area: Rect,
    pub chat_area: Rect,
    pub composer_area: Rect,
    pub clipboard: String,
    pub dragging: bool,
    pub connect: Option<ConnectWizard>,
    pub connect_choice_area: Rect,
    pub model_picker: Option<ModelPicker>,
    pub agent_picker: Option<usize>,
    pub recovery_picker: Option<RecoveryPicker>,
    pub popup_choice_area: Rect,
    pub popup_choice_start: usize,
    pub popup_selected: usize,
    pub animation_frame: usize,
}

pub struct ConnectWizard {
    pub step: ConnectStep,
    pub input: String,
    pub provider_id: String,
    pub name: String,
    pub endpoint: String,
    pub protocol: String,
    pub credential: Credential,
    pub secret: String,
    pub provider_selected: usize,
    pub credential_selected: usize,
}

pub const CONNECT_PROVIDER_NAMES: [&str; 5] = [
    "OpenAI",
    "Anthropic",
    "Google Gemini",
    "OpenRouter",
    "Custom OpenAI-compatible",
];
pub const CONNECT_CREDENTIAL_NAMES: [&str; 3] = [
    "OS keyring (recommended)",
    "Environment variable",
    "Private plaintext file",
];

impl Drop for ConnectWizard {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

#[derive(Clone, Copy)]
pub enum ConnectStep {
    Provider,
    Id,
    Name,
    Endpoint,
    Credential,
    Env,
    SecretKeyring,
    SecretFile,
    PlaintextConfirm,
}

pub struct ModelPicker {
    pub models: Vec<String>,
    pub selected: usize,
    pub scroll: usize,
}

pub struct RecoveryPicker {
    pub entries: Vec<crate::storage::RecoveryEntry>,
    pub selected: usize,
}

impl App {
    pub fn selection_label(&self) -> Option<String> {
        let lines = self.document.selected_lines()?;
        Some(if lines.start() == lines.end() {
            format!("Selected line {}", lines.start())
        } else {
            format!("Selected lines {}:{}", lines.start(), lines.end())
        })
    }

    pub fn selected_attachment(&self) -> Option<Attachment> {
        let range = self.document.selection()?;
        Some(Attachment {
            revision: self.document.revision,
            text: self.document.text[range.clone()].into(),
            range,
            label: self.selection_label()?,
        })
    }

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
            live: crate::live::Live::default(),
            document,
            theme: Theme::named(&config.theme),
            config,
            bindings,
            focus: Focus::Editor,
            workspace,
            messages: vec![],
            composer: String::new(),
            composer_cursor: 0,
            composer_anchor: None,
            message_selection: None,
            chat_view_rows: vec![],
            chat_history_area: Rect::default(),
            composer_view_rows: vec![],
            chat_drag: None,
            prompt_history: vec![],
            prompt_history_index: None,
            prompt_history_draft: String::new(),
            completion: 0,
            chat_scroll: 0,
            follow_chat: true,
            agents: Agent::defaults()?,
            agent: 0,
            attachment: None,
            proposal: None,
            review: None,
            review_view_rows: vec![],
            review_row_hunks: vec![],
            review_buttons: vec![],
            review_gutter: 0,
            explorer: None,
            pending: None,
            save_prompt: None,
            status: "Tab switches panes · Ctrl+S saves · Ctrl+Q exits · /help in chat".into(),
            level: Level::Info,
            quit: false,
            editor_scroll: 0,
            editor_horizontal: 0,
            editor_view_rows: vec![],
            follow_editor: true,
            editor_area: Rect::default(),
            chat_area: Rect::default(),
            composer_area: Rect::default(),
            clipboard: String::new(),
            dragging: false,
            connect: None,
            connect_choice_area: Rect::default(),
            model_picker: None,
            agent_picker: None,
            recovery_picker: None,
            popup_choice_area: Rect::default(),
            popup_choice_start: 0,
            popup_selected: 0,
            animation_frame: 0,
        };
        if app.config.model == "mock" {
            app.message("system", "Mock backend — no network requests or AI inference. Use /demo or /rewrite to test safe edit review. /models selects a live provider.");
        } else {
            app.message("system", format!("Live genai model: {}. Prompts, chat history, current selections, pinned attachments, and approved file reads are sent to this provider. A selection is included automatically; /attach pins context for later prompts; /cancel stops generation.", app.config.model));
        }
        if let Err(e) = app.load_agents() {
            app.notify(
                Level::Warning,
                format!("Some custom agents were not loaded: {e:#}"),
            );
        }
        if let Some(index) = app
            .agents
            .iter()
            .position(|agent| agent.name.eq_ignore_ascii_case(&app.config.agent))
        {
            app.agent = index;
        } else {
            app.config.agent = app.agents[0].name.clone();
            app.notify(
                Level::Warning,
                format!("Saved agent is unavailable; using {}", app.config.agent),
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
            role: role.into(),
            text: text.into(),
            agent: matches!(role, "assistant" | "mock")
                .then(|| self.agents[self.agent].name.clone()),
            model: match role {
                "assistant" => Some(self.config.model.clone()),
                "mock" => Some("mock".into()),
                _ => None,
            },
            usage: None,
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
        self.sync_review();
        let cursor = self.document.cursor;
        self.key_inner(key);
        self.sync_review();
        if self.document.cursor != cursor {
            self.select_change_at_cursor();
        }
    }

    fn select_change_at_cursor(&mut self) {
        if let Some(review) = &mut self.review {
            review.select_at(self.document.cursor);
        }
    }

    fn key_inner(&mut self, key: KeyEvent) {
        // Escape is the universal, highest-priority interrupt while an agent is active.
        if key.code == KeyCode::Esc && self.busy() {
            self.cancel_live();
            return;
        }
        if self.save_prompt.is_some() {
            self.save_prompt_key(key);
            return;
        }
        if self.connect.is_some() {
            self.connect_key(key);
            return;
        }
        if self.model_picker.is_some() {
            self.model_picker_key(key);
            return;
        }
        if self.agent_picker.is_some() {
            self.agent_picker_key(key);
            return;
        }
        if self.recovery_picker.is_some() {
            self.recovery_picker_key(key);
            return;
        }
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
        if ctrl
            && key.code == KeyCode::Char('c')
            && !key.modifiers.contains(KeyModifiers::SHIFT)
            && (self.focus == Focus::Chat || self.live.permission.is_some())
            && self.busy()
            && (self.live.permission.is_some() || self.chat_selected_text().is_none())
        {
            self.cancel_live();
            return;
        }
        if self.live.permission.is_some() {
            self.permission_key(key);
            return;
        }
        if (self.proposal.is_some() || self.review.is_some()) && self.change_review_key(key) {
            return;
        }
        if key.code == KeyCode::BackTab
            || key.code == KeyCode::Tab && key.modifiers == KeyModifiers::SHIFT
        {
            if let Err(error) = self.rotate_agent_mode() {
                self.notify(
                    Level::Error,
                    format!("Could not remember agent/mode: {error:#}"),
                );
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
        if ctrl && key.code == KeyCode::Char('g') {
            if let Err(error) = self.toggle_agent_mode() {
                self.notify(Level::Error, format!("Could not remember mode: {error:#}"));
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
            if matches!(
                action.as_str(),
                "delete" | "backspace" | "kill_line" | "cut" | "paste" | "undo" | "redo"
            ) {
                self.discard_proposal_for_manual_edit();
            }
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
            KeyCode::Backspace => {
                self.discard_proposal_for_manual_edit();
                self.document.backspace();
            }
            KeyCode::Delete => {
                self.discard_proposal_for_manual_edit();
                self.document.delete();
            }
            KeyCode::Enter => {
                self.discard_proposal_for_manual_edit();
                self.document.insert("\n");
            }
            KeyCode::Esc => self.document.anchor = None,
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.discard_proposal_for_manual_edit();
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
            "kill_line" => self.document.kill_line(),
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
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let action = self.bindings.editor.get(&key_name(key)).map(String::as_str);
        if action == Some("copy") || ctrl && matches!(key.code, KeyCode::Char('c' | 'C')) {
            if let Some(text) = self.chat_selected_text() {
                self.clipboard = text;
                self.notify(Level::Success, "Copied to Queditor's internal clipboard");
            }
            return;
        }
        if action == Some("cut") {
            if self.message_selection.is_none()
                && let Some(text) = self.chat_selected_text()
            {
                self.clipboard = text;
                self.compose_insert("");
            }
            return;
        }
        if action == Some("paste") {
            self.compose_insert(&self.clipboard.clone());
            return;
        }
        if ctrl && shift && matches!(key.code, KeyCode::Char('a' | 'A')) {
            if let Some(selection) = &mut self.message_selection {
                selection.anchor = 0;
                selection.cursor = self
                    .messages
                    .get(selection.message)
                    .map_or(0, |message| display_text(&message.text).len());
            } else {
                self.composer_anchor = Some(0);
                self.composer_cursor = self.composer.len();
            }
            return;
        }
        let old_cursor = self.composer_cursor;
        let old_selection = self.composer_selection();
        let moving = matches!(
            key.code,
            KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Up
                | KeyCode::Down
        ) || ctrl && matches!(key.code, KeyCode::Char('a' | 'e'));
        if moving {
            self.message_selection = None;
            if shift {
                self.composer_anchor.get_or_insert(old_cursor);
            } else {
                self.composer_anchor = None;
            }
        }
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.compose_insert("\n")
            }
            KeyCode::Char('j') if ctrl => self.compose_insert("\n"),
            KeyCode::Char('a') if ctrl => {
                self.composer_cursor = self.composer[..self.composer_cursor]
                    .rfind('\n')
                    .map_or(0, |index| index + 1);
            }
            KeyCode::Char('e') if ctrl => self.composer_cursor = self.composer_line_end(),
            KeyCode::Char('k') if ctrl => {
                if self.composer_selection().is_some() {
                    self.compose_insert("");
                    return;
                }
                let end = self.composer_line_end();
                if end > self.composer_cursor {
                    self.stop_browsing_prompt_history();
                    self.composer.replace_range(self.composer_cursor..end, "");
                    self.completion = 0;
                }
            }
            KeyCode::Enter => {
                if !self.completions().is_empty() {
                    self.complete();
                }
                self.submit();
            }
            KeyCode::Left => {
                self.composer_cursor = if !shift {
                    old_selection.as_ref().map(|range| range.start)
                } else {
                    None
                }
                .unwrap_or_else(|| previous(&self.composer, self.composer_cursor))
            }
            KeyCode::Right => {
                self.composer_cursor = if !shift {
                    old_selection.as_ref().map(|range| range.end)
                } else {
                    None
                }
                .unwrap_or_else(|| next(&self.composer, self.composer_cursor))
            }
            KeyCode::Home => self.composer_cursor = 0,
            KeyCode::End => self.composer_cursor = self.composer.len(),
            KeyCode::Backspace | KeyCode::Delete if self.composer_selection().is_some() => {
                self.compose_insert("");
            }
            KeyCode::Backspace if self.composer_cursor > 0 => {
                self.stop_browsing_prompt_history();
                let start = previous(&self.composer, self.composer_cursor);
                self.composer.replace_range(start..self.composer_cursor, "");
                self.composer_cursor = start;
            }
            KeyCode::Delete if self.composer_cursor < self.composer.len() => {
                self.composer.replace_range(
                    self.composer_cursor..next(&self.composer, self.composer_cursor),
                    "",
                );
                self.stop_browsing_prompt_history();
            }
            KeyCode::Up if !shift && !self.completions().is_empty() => {
                self.completion = self.completion.saturating_sub(1)
            }
            KeyCode::Down if !shift && !self.completions().is_empty() => {
                self.completion = (self.completion + 1).min(self.completions().len() - 1)
            }
            KeyCode::Up => self.composer_vertical(-1, shift),
            KeyCode::Down => self.composer_vertical(1, shift),
            KeyCode::PageUp => {
                self.follow_chat = false;
                self.chat_scroll = self.chat_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.chat_scroll += 10;
            }
            KeyCode::Esc => {
                if self.message_selection.take().is_some() || self.composer_anchor.take().is_some()
                {
                    return;
                }
                self.composer.clear();
                self.composer_cursor = 0;
                self.stop_browsing_prompt_history();
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

    fn composer_line_end(&self) -> usize {
        self.composer[self.composer_cursor..]
            .find('\n')
            .map_or(self.composer.len(), |index| self.composer_cursor + index)
    }

    pub fn composer_selection(&self) -> Option<Range<usize>> {
        self.composer_anchor
            .filter(|anchor| *anchor != self.composer_cursor)
            .map(|anchor| anchor.min(self.composer_cursor)..anchor.max(self.composer_cursor))
    }

    fn chat_selected_text(&self) -> Option<String> {
        if let Some(selection) = &self.message_selection {
            let text = display_text(&self.messages.get(selection.message)?.text);
            let range =
                selection.anchor.min(selection.cursor)..selection.anchor.max(selection.cursor);
            return (!range.is_empty())
                .then(|| text.get(range).map(str::to_owned))
                .flatten();
        }
        self.composer_selection()
            .map(|range| self.composer[range].to_owned())
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
            self.composer_anchor = None;
            self.completion = 0;
        }
    }

    pub fn compose_insert(&mut self, text: &str) {
        self.stop_browsing_prompt_history();
        self.message_selection = None;
        let range = self
            .composer_selection()
            .unwrap_or(self.composer_cursor..self.composer_cursor);
        let end = range.start + text.len();
        self.composer.replace_range(range, text);
        self.composer_anchor = None;
        self.composer_cursor = unicode_segmentation::UnicodeSegmentation::grapheme_indices(
            self.composer.as_str(),
            true,
        )
        .map(|(i, _)| i)
        .find(|i| *i >= end)
        .unwrap_or(self.composer.len());
        self.completion = 0;
    }

    fn composer_vertical(&mut self, delta: isize, selecting: bool) {
        let width = usize::from(self.composer_area.width).max(1);
        let rows = composer_rows(&self.composer, width);
        let row = rows
            .iter()
            .rposition(|row| row.start <= self.composer_cursor)
            .unwrap_or(0);
        if delta < 0 && row == 0 {
            if !selecting {
                self.previous_prompt();
            }
            return;
        }
        if delta > 0 && row + 1 == rows.len() {
            if !selecting {
                self.next_prompt();
            }
            return;
        }
        let target = row.saturating_add_signed(delta).min(rows.len() - 1);
        let column = composer_display_width(&self.composer[rows[row].start..self.composer_cursor]);
        self.composer_cursor = composer_position_at_display(&self.composer, rows[target], column);
    }

    fn previous_prompt(&mut self) {
        if self.prompt_history.is_empty() {
            return;
        }
        let index = match self.prompt_history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.prompt_history_draft = self.composer.clone();
                self.prompt_history.len() - 1
            }
        };
        self.load_prompt_history(index);
    }

    fn next_prompt(&mut self) {
        let Some(index) = self.prompt_history_index else {
            return;
        };
        if index + 1 < self.prompt_history.len() {
            self.load_prompt_history(index + 1);
        } else {
            self.composer = std::mem::take(&mut self.prompt_history_draft);
            self.composer_cursor = self.composer.len();
            self.composer_anchor = None;
            self.prompt_history_index = None;
        }
    }

    fn load_prompt_history(&mut self, index: usize) {
        self.composer.clone_from(&self.prompt_history[index]);
        self.composer_cursor = self.composer.len();
        self.composer_anchor = None;
        self.prompt_history_index = Some(index);
        self.completion = 0;
    }

    fn stop_browsing_prompt_history(&mut self) {
        self.prompt_history_index = None;
        self.prompt_history_draft.clear();
    }

    pub(crate) fn clear_prompt_history(&mut self) {
        self.message_selection = None;
        self.composer_anchor = None;
        self.chat_view_rows.clear();
        self.prompt_history.clear();
        self.stop_browsing_prompt_history();
    }

    pub(crate) fn rebuild_prompt_history(&mut self) {
        self.message_selection = None;
        self.composer_anchor = None;
        self.chat_view_rows.clear();
        self.prompt_history = self
            .messages
            .iter()
            .filter(|message| message.role == "you")
            .map(|message| message.text.clone())
            .collect();
        self.stop_browsing_prompt_history();
    }

    pub fn paste(&mut self, text: &str) {
        self.sync_review();
        self.paste_inner(text);
        self.sync_review();
    }

    fn paste_inner(&mut self, text: &str) {
        if self.save_prompt.is_some() {
            self.save_prompt_insert(text);
            return;
        }
        if self.connect.is_some() {
            self.connect_paste(text);
            return;
        }
        if self.pending.is_some()
            || self.model_picker.is_some()
            || self.agent_picker.is_some()
            || self.recovery_picker.is_some()
        {
            return;
        }
        if self.focus == Focus::Editor {
            self.discard_proposal_for_manual_edit();
            self.document.insert(text);
            self.follow_editor = true;
        } else if self.explorer.is_none() {
            self.compose_insert(text);
        }
    }

    fn connect_paste(&mut self, text: &str) {
        let Some(wizard) = self.connect.as_mut() else {
            return;
        };
        let target = match wizard.step {
            ConnectStep::SecretKeyring | ConnectStep::SecretFile => &mut wizard.secret,
            ConnectStep::Id | ConnectStep::Name | ConnectStep::Endpoint | ConnectStep::Env => {
                &mut wizard.input
            }
            ConnectStep::Provider | ConnectStep::Credential | ConnectStep::PlaintextConfirm => {
                return;
            }
        };
        for character in text.trim().chars() {
            if matches!(character, '\r' | '\n') || character.is_control() {
                continue;
            }
            if target.len() + character.len_utf8() > 2048 {
                break;
            }
            target.push(character);
        }
    }

    pub fn submit(&mut self) {
        let input = self.composer.trim().to_owned();
        if input.is_empty() {
            return;
        }
        if !input.starts_with('/') && self.busy() {
            self.notify(
                Level::Warning,
                "Agent is running; cancel before submitting another prompt",
            );
            return;
        }
        if input.starts_with('/') {
            if let Err(e) = self.command(&input) {
                self.notify(Level::Error, format!("{e:#}"));
                return; // Keep failed input for correction.
            }
        } else {
            self.message("you", input.clone());
            if self.config.model != "mock" {
                if let Err(e) = self.start_live(&input, false) {
                    self.notify(Level::Error, format!("{e:#}"));
                    return;
                }
            } else {
                let context = self
                    .attachment
                    .as_ref()
                    .map(|a| format!(" Attached {} ({} bytes).", a.label, a.text.len()))
                    .unwrap_or_default();
                self.message("mock", format!("{} is selected.{context} Mock mode is selected. /models <provider::model> enables live AI. /demo previews an uppercase edit; /rewrite <text> previews a replacement.", self.agents[self.agent].name));
            }
        }
        self.prompt_history.push(input);
        self.stop_browsing_prompt_history();
        self.composer.clear();
        self.composer_cursor = 0;
        self.composer_anchor = None;
        self.message_selection = None;
        self.completion = 0;
    }

    pub fn command(&mut self, input: &str) -> Result<()> {
        self.sync_review();
        let result = self.command_inner(input);
        self.sync_review();
        result
    }

    fn command_inner(&mut self, input: &str) -> Result<()> {
        let args = shlex::split(input).context("Unclosed quote in command")?;
        let arg = args.get(1).map(String::as_str);
        match args.first().map(String::as_str).unwrap_or("") {
            "/quit" | "/exit" => self.request(Pending::Quit),
            "/save" => {
                let path = arg.map(|p| self.resolve(p));
                self.request_save(path.as_deref())?;
            }
            "/open" => {
                if let Some(p) = arg {
                    self.request(Pending::Open(self.resolve(p)));
                } else {
                    self.show_explorer(self.workspace.clone())?;
                }
            }
            "/new" => {
                self.cancel_live();
                self.checkpoint_state()?;
                self.new_session();
            }
            "/models" => {
                if self.busy() {
                    bail!("Cancel the current agent before changing model");
                }
                if let Some(model) = arg {
                    self.config.validate_model_ref(model)?;
                    self.select_model(model.to_owned())?;
                } else {
                    self.open_model_picker();
                }
            }
            "/connect" => self.connect_command(&args)?,
            "/compact" => {
                self.start_live("Summarize the conversation for continuation.", true)?;
            }
            "/cancel" => self.cancel_live(),
            "/permissions" => self.permissions_command(&args)?,
            "/sessions" | "/resume" | "/recover" => self.state_command(&args)?,
            "/cd" => {
                if self.busy() {
                    bail!("Cancel the current agent before changing workspace");
                }
                let path = self
                    .resolve(arg.context("Usage: /cd <path>")?)
                    .canonicalize()?;
                if !path.is_dir() {
                    bail!("Not a directory");
                }
                self.workspace = path;
                self.live.permissions.clear_session();
                self.notify(
                    Level::Success,
                    format!(
                        "Workspace: {} · temporary grants cleared",
                        self.workspace.display()
                    ),
                );
            }
            "/ls" => {
                let path = arg
                    .map(|p| self.resolve(p))
                    .unwrap_or_else(|| self.workspace.clone());
                let e = Explorer::load(path)?;
                self.message(
                    "files",
                    e.entries
                        .iter()
                        .map(|e| format!("{}{}", e.name, if e.directory { "/" } else { "" }))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            "/files" => self.show_explorer(
                arg.map(|p| self.resolve(p))
                    .unwrap_or_else(|| self.workspace.clone()),
            )?,
            "/theme" => {
                let name = arg.context("Usage: /theme mocha|daylight")?;
                if !matches!(name, "mocha" | "daylight") {
                    bail!("Unknown theme");
                }
                self.config.theme = name.into();
                self.theme = Theme::named(name);
                self.notify(Level::Success, format!("Theme: {name} (this run)"));
            }
            "/agents" => {
                if let Some(name) = arg {
                    let index = self
                        .agents
                        .iter()
                        .position(|a| a.name.eq_ignore_ascii_case(name))
                        .context("Unknown agent")?;
                    self.select_agent(index)?;
                } else {
                    self.agent_picker = Some(self.agent);
                }
            }
            "/mode" => {
                let mode = match arg {
                    Some("chat") => AgentMode::Chat,
                    Some("agent") => AgentMode::Agent,
                    _ => bail!("Usage: /mode chat|agent"),
                };
                self.select_agent_mode(mode)?;
            }
            "/changes" => match arg {
                Some("expand" | "collapse") => {
                    let review = self
                        .review
                        .as_mut()
                        .context("No applied agent changes to review")?;
                    review.expanded = arg == Some("expand");
                    self.follow_editor = true;
                }
                Some("next") => self.select_next_change(),
                Some("prev") => self.select_previous_change(),
                Some("keep") => self.accept_current_change(),
                Some("reject" | "undo") => self.reject_current_change(),
                Some("keep-all") => self.keep_all_changes(),
                Some("reject-all" | "undo-all") => self.reject_proposal(),
                _ => bail!(
                    "Usage: /changes next|prev|keep|undo|keep-all|undo-all|expand|collapse (reject aliases undo)"
                ),
            },
            "/attach" => {
                if arg == Some("clear") {
                    self.attachment = None;
                } else {
                    let range = if arg == Some("document") {
                        0..self.document.text.len()
                    } else {
                        self.document
                            .selection()
                            .context("Select editor text first, or use /attach document")?
                    };
                    self.attachment = Some(Attachment {
                        revision: self.document.revision,
                        text: self.document.text[range.clone()].into(),
                        range,
                        label: if arg == Some("document") {
                            "document"
                        } else {
                            "selection"
                        }
                        .into(),
                    });
                }
                self.notify(
                    Level::Success,
                    if self.attachment.is_some() {
                        "Context snapshot attached"
                    } else {
                        "Context cleared"
                    },
                );
            }
            "/demo" | "/rewrite" => {
                if self.busy() {
                    bail!("Cancel the current agent before creating a mock proposal");
                }
                if self.proposal.is_some() || self.review.is_some() {
                    bail!("Resolve the current proposal first");
                }
                let range = if let Some(a) = &self.attachment {
                    if a.revision != self.document.revision {
                        bail!("Attached context is stale; reattach current text");
                    }
                    a.range.clone()
                } else {
                    self.document
                        .selection()
                        .unwrap_or(0..self.document.text.len())
                };
                let original = self.document.text[range.clone()].to_owned();
                let replacement = if args[0] == "/demo" {
                    original.to_uppercase()
                } else {
                    args.get(1..)
                        .filter(|a| !a.is_empty())
                        .context("Usage: /rewrite <replacement text>")?
                        .join(" ")
                };
                if original == replacement {
                    bail!("Mock proposal has no changes");
                }
                self.proposal = Some(Proposal::new(
                    true,
                    self.document.revision,
                    range,
                    original,
                    replacement,
                ));
                self.editor_scroll = 0;
                self.notify(
                    Level::Info,
                    "Mock changes ready · Alt+[ / Alt+] navigate · Ctrl+Enter keeps · Ctrl+Backspace rejects",
                );
            }
            "/find" => {
                let needle = args
                    .get(1..)
                    .filter(|a| !a.is_empty())
                    .context("Usage: /find <text>")?
                    .join(" ");
                if needle.is_empty() {
                    bail!("Search cannot be empty");
                }
                let start = self.document.cursor;
                let pos = self.document.text[start..]
                    .find(&needle)
                    .map(|i| start + i)
                    .or_else(|| self.document.text[..start].find(&needle))
                    .context("Text not found")?;
                self.document.move_to(pos, false);
                self.document.move_to(pos + needle.len(), true);
                self.focus = Focus::Editor;
                self.follow_editor = true;
            }
            "/replace" => {
                if args.len() != 3 || args[1].is_empty() {
                    bail!("Usage: /replace <old> <new> (quote spaces)");
                }
                self.discard_proposal_for_manual_edit();
                let result = self.document.text.replace(&args[1], &args[2]);
                self.document.replace(0..self.document.text.len(), &result);
                self.follow_editor = true;
                self.notify(Level::Success, "Replace-all complete; Ctrl+Z undoes it");
            }
            "/keys" => {
                let mut text = format!(
                    "Profile: {}\nGlobal: Tab focus; Shift+Tab next agent/mode; Ctrl+G next mode; Ctrl+S save; Ctrl+Q quit\nChat: Enter submit; Ctrl+J newline; Tab complete; Ctrl+A/E line start/end; Ctrl+K delete to line end; Shift+arrows select prompt; Ctrl+Shift+A selects all in prompt/clicked message; drag message text to select and copy\nEditor: arrows, Shift+arrows select, Home/End, PgUp/PgDown\nCopy/cut/paste bindings also work in the prompt; messages are read-only. Shortcuts use an internal clipboard; terminal paste is supported.\n",
                    self.config.keybindings
                );
                for (key, action) in &self.bindings.editor {
                    text.push_str(&format!("{key}: {action}\n"));
                }
                self.message("keys", text);
            }
            "/version" => self.message("version", concat!("queditor ", env!("CARGO_PKG_VERSION"))),
            "/help" => self.message(
                "help",
                COMMANDS
                    .iter()
                    .map(|(c, d)| format!("{c} — {d}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => bail!("Unknown command; use /help"),
        }
        Ok(())
    }

    fn connect_command(&mut self, args: &[String]) -> Result<()> {
        if self.busy() || self.live.discovery.is_some() {
            bail!("Cancel or wait for the current provider operation first");
        }
        if args.get(1).is_some_and(|arg| arg == "delete") {
            let id = args.get(2).context("Usage: /connect delete <provider>")?;
            let provider = self
                .config
                .providers
                .remove(id)
                .context("Unknown provider")?;
            let previous_model = self.config.model.clone();
            if self.config.model.starts_with(&format!("{id}::")) {
                self.config.model = "mock".into();
            }
            if let Err(error) = self.config.save() {
                self.config.providers.insert(id.clone(), provider);
                self.config.model = previous_model;
                return Err(error);
            }
            crate::credentials::delete(&provider.credential)?;
            self.notify(
                Level::Success,
                format!("Provider {id} and its saved credential were removed"),
            );
            return Ok(());
        }
        self.connect = Some(ConnectWizard {
            step: ConnectStep::Provider,
            input: String::new(),
            provider_id: String::new(),
            name: String::new(),
            endpoint: String::new(),
            protocol: "openai_compatible".into(),
            credential: Credential::default(),
            secret: String::new(),
            provider_selected: 0,
            credential_selected: 0,
        });
        Ok(())
    }

    pub fn open_model_picker(&mut self) {
        let mut models = vec!["mock".to_owned()];
        models.extend(self.config.models.iter().filter(|m| *m != "mock").cloned());
        for (id, provider) in &self.config.providers {
            models.extend(provider.models.iter().map(|model| format!("{id}::{model}")));
        }
        models.sort();
        models.dedup();
        if let Some(index) = models.iter().position(|model| model == &self.config.model) {
            models.swap(0, index);
        }
        self.model_picker = Some(ModelPicker {
            models,
            selected: 0,
            scroll: 0,
        });
    }

    fn model_picker_key(&mut self, key: KeyEvent) {
        let Some(picker) = self.model_picker.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.model_picker = None,
            KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Down => {
                picker.selected = (picker.selected + 1).min(picker.models.len().saturating_sub(1))
            }
            KeyCode::Enter => {
                let model = picker.models[picker.selected].clone();
                self.model_picker = None;
                if let Err(error) = self.select_model(model) {
                    self.notify(Level::Error, format!("Could not remember model: {error:#}"));
                }
            }
            _ => {}
        }
    }

    fn select_model(&mut self, model: String) -> Result<()> {
        let previous = std::mem::replace(&mut self.config.model, model.clone());
        if let Err(error) = self.config.save() {
            self.config.model = previous;
            return Err(error);
        }
        self.notify(
            Level::Success,
            format!("Model selected and remembered: {model}"),
        );
        Ok(())
    }

    fn agent_picker_key(&mut self, key: KeyEvent) {
        let Some(selected) = self.agent_picker.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.agent_picker = None,
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected = (*selected + 1).min(self.agents.len().saturating_sub(1)),
            KeyCode::Enter => {
                let index = *selected;
                self.select_agent_picker_at(index);
            }
            _ => {}
        }
    }

    fn recovery_picker_key(&mut self, key: KeyEvent) {
        let Some(picker) = self.recovery_picker.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.recovery_picker = None,
            KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Down => {
                picker.selected = (picker.selected + 1).min(picker.entries.len().saturating_sub(1))
            }
            KeyCode::Enter => {
                let selected = picker.selected;
                self.select_recovery_picker_at(selected);
            }
            _ => {}
        }
    }

    fn select_recovery_picker_at(&mut self, selected: usize) {
        let Some(entry) = self
            .recovery_picker
            .as_ref()
            .and_then(|p| p.entries.get(selected))
        else {
            return;
        };
        let id = entry.id.clone();
        self.recovery_picker = None;
        self.popup_choice_area = Rect::default();
        self.request(Pending::Recover(id));
    }

    fn select_agent_picker_at(&mut self, selected: usize) {
        if self.agent_picker.is_none() || selected >= self.agents.len() {
            return;
        }
        if let Err(error) = self.select_agent(selected) {
            self.notify(Level::Error, format!("Could not remember agent: {error:#}"));
        } else {
            self.agent_picker = None;
        }
    }

    fn select_agent(&mut self, index: usize) -> Result<()> {
        let name = self.remember_agent_selection(index, None)?;
        self.notify(
            Level::Success,
            format!("Agent selected and remembered: {name}"),
        );
        Ok(())
    }

    /// Persist an agent/mode pair atomically; failed saves leave both unchanged.
    fn remember_agent_selection(
        &mut self,
        index: usize,
        mode: Option<AgentMode>,
    ) -> Result<String> {
        let name = self
            .agents
            .get(index)
            .context("Unknown agent")?
            .name
            .clone();
        let mut config = self.config.clone();
        config.agent = name.clone();
        if let Some(mode) = mode {
            config.agent_modes.insert(name.clone(), mode);
        }
        config.save()?;
        self.config = config;
        self.agent = index;
        Ok(name)
    }

    pub fn agent_mode(&self) -> AgentMode {
        self.config
            .agent_modes
            .get(&self.agents[self.agent].name)
            .copied()
            .unwrap_or_default()
    }

    fn select_agent_mode(&mut self, mode: AgentMode) -> Result<()> {
        self.select_agent_with_mode(self.agent, mode)
    }

    fn select_agent_with_mode(&mut self, index: usize, mode: AgentMode) -> Result<()> {
        let name = self.remember_agent_selection(index, Some(mode))?;
        self.notify(
            Level::Success,
            format!(
                "{} mode selected and remembered for {name}{}",
                mode.label(),
                if mode == AgentMode::Agent {
                    " · live edits apply and save before Keep/Undo review"
                } else {
                    " · read-only"
                }
            ),
        );
        Ok(())
    }

    fn toggle_agent_mode(&mut self) -> Result<()> {
        self.select_agent_mode(self.agent_mode().next())
    }

    fn rotate_agent_mode(&mut self) -> Result<()> {
        let current = self.agents.get(self.agent).context("No selected agent")?;
        let mode = self
            .config
            .agent_modes
            .get(&current.name)
            .copied()
            .unwrap_or_default();
        let next_mode = mode.next();
        let next_agent = if next_mode == AgentMode::ALL[0] {
            (self.agent + 1) % self.agents.len()
        } else {
            self.agent
        };
        // On an agent boundary start at the first mode, not its remembered mode.
        self.select_agent_with_mode(next_agent, next_mode)
    }

    fn connect_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            if let Some(mut wizard) = self.connect.take() {
                wizard.secret.clear();
            }
            self.notify(Level::Info, "Provider setup cancelled");
            return;
        }
        let Some(wizard) = self.connect.as_mut() else {
            return;
        };
        match wizard.step {
            ConnectStep::Provider => match key.code {
                KeyCode::Up => {
                    wizard.provider_selected = wizard.provider_selected.saturating_sub(1)
                }
                KeyCode::Down => {
                    wizard.provider_selected =
                        (wizard.provider_selected + 1).min(CONNECT_PROVIDER_NAMES.len() - 1)
                }
                KeyCode::Enter => self.select_connect_provider(),
                KeyCode::Char('o') => self.select_connect_provider_at(0),
                KeyCode::Char('a') => self.select_connect_provider_at(1),
                KeyCode::Char('g') => self.select_connect_provider_at(2),
                KeyCode::Char('r') => self.select_connect_provider_at(3),
                KeyCode::Char('c') => self.select_connect_provider_at(4),
                _ => {}
            },
            ConnectStep::Credential => match key.code {
                KeyCode::Up => {
                    wizard.credential_selected = wizard.credential_selected.saturating_sub(1)
                }
                KeyCode::Down => {
                    wizard.credential_selected =
                        (wizard.credential_selected + 1).min(CONNECT_CREDENTIAL_NAMES.len() - 1)
                }
                KeyCode::Enter => self.select_connect_credential(),
                KeyCode::Char('k') => self.select_connect_credential_at(0),
                KeyCode::Char('e') => self.select_connect_credential_at(1),
                KeyCode::Char('f') => self.select_connect_credential_at(2),
                _ => {}
            },
            ConnectStep::PlaintextConfirm => match key.code {
                KeyCode::Char('y') => wizard.step = ConnectStep::SecretFile,
                KeyCode::Char('n') => wizard.step = ConnectStep::Credential,
                KeyCode::Up => self.popup_selected = self.popup_selected.saturating_sub(1),
                KeyCode::Down => self.popup_selected = (self.popup_selected + 1).min(1),
                KeyCode::Enter if self.popup_selected == 0 => wizard.step = ConnectStep::SecretFile,
                KeyCode::Enter => wizard.step = ConnectStep::Credential,
                _ => {}
            },
            _ => match key.code {
                KeyCode::Backspace => {
                    let target = if matches!(
                        wizard.step,
                        ConnectStep::SecretKeyring | ConnectStep::SecretFile
                    ) {
                        &mut wizard.secret
                    } else {
                        &mut wizard.input
                    };
                    target.pop();
                }
                KeyCode::Enter => self.advance_connect(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let target = if matches!(
                        wizard.step,
                        ConnectStep::SecretKeyring | ConnectStep::SecretFile
                    ) {
                        &mut wizard.secret
                    } else {
                        &mut wizard.input
                    };
                    if target.len() < 2048 {
                        target.push(c);
                    }
                }
                _ => {}
            },
        }
    }

    fn select_connect_provider(&mut self) {
        let selected = self
            .connect
            .as_ref()
            .map(|wizard| wizard.provider_selected)
            .unwrap_or_default();
        self.select_connect_provider_at(selected);
    }

    fn select_connect_provider_at(&mut self, selected: usize) {
        let Some(wizard) = self.connect.as_mut() else {
            return;
        };
        wizard.provider_selected = selected.min(CONNECT_PROVIDER_NAMES.len() - 1);
        match wizard.provider_selected {
            0 => {
                wizard.provider_id = "openai".into();
                wizard.name = "OpenAI".into();
                wizard.endpoint = "https://api.openai.com/v1/".into();
                wizard.protocol = "openai".into();
                wizard.step = ConnectStep::Credential;
            }
            1 => {
                wizard.provider_id = "anthropic".into();
                wizard.name = "Anthropic".into();
                wizard.endpoint = "https://api.anthropic.com/".into();
                wizard.protocol = "anthropic".into();
                wizard.step = ConnectStep::Credential;
            }
            2 => {
                wizard.provider_id = "gemini".into();
                wizard.name = "Google Gemini".into();
                wizard.endpoint = "https://generativelanguage.googleapis.com/v1beta/".into();
                wizard.protocol = "gemini".into();
                wizard.step = ConnectStep::Credential;
            }
            3 => {
                wizard.provider_id = "openrouter".into();
                wizard.name = "OpenRouter".into();
                wizard.endpoint = "https://openrouter.ai/api/v1/".into();
                wizard.protocol = "openrouter".into();
                wizard.step = ConnectStep::Credential;
            }
            _ => wizard.step = ConnectStep::Id,
        }
    }

    fn select_connect_credential(&mut self) {
        let selected = self
            .connect
            .as_ref()
            .map(|wizard| wizard.credential_selected)
            .unwrap_or_default();
        self.select_connect_credential_at(selected);
    }

    fn select_connect_credential_at(&mut self, selected: usize) {
        let Some(wizard) = self.connect.as_mut() else {
            return;
        };
        wizard.credential_selected = selected.min(CONNECT_CREDENTIAL_NAMES.len() - 1);
        wizard.step = match wizard.credential_selected {
            0 => ConnectStep::SecretKeyring,
            1 => ConnectStep::Env,
            _ => {
                self.popup_selected = 0;
                ConnectStep::PlaintextConfirm
            }
        };
    }

    fn advance_connect(&mut self) {
        let result = (|| -> Result<bool> {
            let wizard = self.connect.as_mut().context("No provider setup")?;
            match wizard.step {
                ConnectStep::Id => {
                    let id = wizard.input.trim();
                    if id.is_empty()
                        || id.len() > 64
                        || !id
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                    {
                        bail!("Use letters, numbers, '-' or '_' for the provider ID");
                    }
                    if self.config.providers.contains_key(id) {
                        bail!("That provider ID already exists");
                    }
                    wizard.provider_id = id.into();
                    wizard.input.clear();
                    wizard.step = ConnectStep::Name;
                }
                ConnectStep::Name => {
                    wizard.name = wizard.input.trim().to_owned();
                    if wizard.name.is_empty() {
                        bail!("Provider name cannot be empty");
                    }
                    wizard.input.clear();
                    wizard.step = ConnectStep::Endpoint;
                }
                ConnectStep::Endpoint => {
                    wizard.endpoint = crate::provider::normalize_endpoint(wizard.input.trim())?;
                    wizard.input.clear();
                    wizard.step = ConnectStep::Credential;
                }
                ConnectStep::Env => {
                    let env = wizard.input.trim();
                    if env.is_empty()
                        || !env
                            .chars()
                            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    {
                        bail!("Use an uppercase environment variable name such as OPENAI_API_KEY");
                    }
                    wizard.credential.env = Some(env.into());
                    return Ok(true);
                }
                ConnectStep::SecretKeyring | ConnectStep::SecretFile => {
                    if wizard.secret.is_empty() {
                        bail!("API key cannot be empty");
                    }
                    return Ok(true);
                }
                _ => {}
            }
            Ok(false)
        })();
        match result {
            Ok(true) => self.finish_connect(),
            Ok(false) => {}
            Err(error) => self.notify(Level::Error, format!("{error:#}")),
        }
    }

    fn finish_connect(&mut self) {
        let result = (|| -> Result<String> {
            let mut wizard = self.connect.take().context("No provider setup")?;
            let id = wizard.provider_id.clone();
            if self.config.providers.contains_key(&id) {
                bail!("Provider {id} already exists; use /connect delete {id} first");
            }
            if matches!(
                wizard.step,
                ConnectStep::SecretKeyring | ConnectStep::SecretFile
            ) {
                let secret = secrecy::SecretString::from(std::mem::take(&mut wizard.secret));
                if matches!(wizard.step, ConnectStep::SecretKeyring) {
                    crate::credentials::save_keyring(&id, &secret)?;
                    wizard.credential.keyring = Some(id.clone());
                } else {
                    crate::credentials::save_file(&id, &secret)?;
                    wizard.credential.file = Some(id.clone());
                }
            }
            let provider = Provider {
                name: std::mem::take(&mut wizard.name),
                protocol: std::mem::take(&mut wizard.protocol),
                base_url: crate::provider::normalize_endpoint(&wizard.endpoint)?,
                credential: std::mem::take(&mut wizard.credential),
                models: vec![],
            };
            self.config.providers.insert(id.clone(), provider);
            if let Err(error) = self.config.save() {
                if let Some(provider) = self.config.providers.remove(&id) {
                    let _ = crate::credentials::delete(&provider.credential);
                }
                bail!("Could not save provider: {error:#}");
            }
            self.discover_provider(id.clone())?;
            Ok(id)
        })();
        if let Err(error) = result {
            self.notify(Level::Error, format!("Provider setup failed: {error:#}"));
        }
    }

    pub fn accept_proposal(&mut self) {
        if self.review.take().is_some() {
            self.notify(
                Level::Success,
                "Agent changes kept; already applied and saved",
            );
            return;
        }
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
            let replacement = p.included_replacement();
            if replacement == p.original {
                self.notify(Level::Info, "No included changes; document unchanged");
                self.editor_scroll = 0;
                self.follow_editor = true;
                return;
            }
            self.document.replace(p.range, &replacement);
            self.attachment = None;
            self.notify(
                Level::Success,
                "Proposal accepted (not saved); Ctrl+Z undoes it",
            );
        }
        self.editor_scroll = 0;
        self.follow_editor = true;
    }

    fn change_review_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char(']') if alt => self.select_next_change(),
            KeyCode::Char('[') if alt => self.select_previous_change(),
            KeyCode::Enter if ctrl => self.accept_current_change(),
            KeyCode::Backspace if ctrl => self.reject_current_change(),
            _ => return false,
        }
        true
    }

    fn select_next_change(&mut self) {
        if let Some(review) = &mut self.review {
            if let Some(index) = (1..=review.hunks.len())
                .map(|step| (review.active + step) % review.hunks.len())
                .find(|&index| !review.hunks[index].resolved)
            {
                review.active = index;
            } else {
                return;
            }
            let range = review.hunks[review.active].range.clone();
            self.document.move_to(range.start, false);
            self.focus = Focus::Editor;
            self.follow_editor = true;
            return;
        }
        let selected = if let Some(proposal) = &mut self.proposal {
            proposal.active_hunk = (proposal.active_hunk + 1) % proposal.hunks.len().max(1);
            Some((proposal.active_hunk + 1, proposal.hunks.len()))
        } else {
            None
        };
        if let Some((active, total)) = selected {
            self.focus = Focus::Editor;
            self.follow_editor = true;
            self.notify(
                Level::Info,
                format!(
                    "Change {} of {} · Ctrl+Enter keep · Ctrl+Backspace reject",
                    active, total
                ),
            );
        }
    }

    fn select_previous_change(&mut self) {
        if let Some(review) = &mut self.review {
            let len = review.hunks.len();
            if let Some(index) = (1..=len)
                .map(|step| (review.active + len - step) % len)
                .find(|&index| !review.hunks[index].resolved)
            {
                review.active = index;
            } else {
                return;
            }
            self.document
                .move_to(review.hunks[review.active].range.start, false);
            self.focus = Focus::Editor;
            self.follow_editor = true;
            return;
        }
        let selected = if let Some(proposal) = &mut self.proposal {
            proposal.active_hunk = proposal
                .active_hunk
                .checked_sub(1)
                .unwrap_or_else(|| proposal.hunks.len().saturating_sub(1));
            Some((proposal.active_hunk + 1, proposal.hunks.len()))
        } else {
            None
        };
        if let Some((active, total)) = selected {
            self.focus = Focus::Editor;
            self.follow_editor = true;
            self.notify(
                Level::Info,
                format!(
                    "Change {} of {} · Ctrl+Enter keep · Ctrl+Backspace reject",
                    active, total
                ),
            );
        }
    }

    fn accept_current_change(&mut self) {
        self.resolve_current_change(true);
    }

    fn reject_current_change(&mut self) {
        self.resolve_current_change(false);
    }

    fn resolve_current_change(&mut self, included: bool) {
        if self.review.is_some() {
            self.resolve_applied_changes(included, false);
            return;
        }
        let complete = if let Some(proposal) = &mut self.proposal {
            if let Some(hunk) = proposal.hunks.get_mut(proposal.active_hunk) {
                hunk.included = included;
                hunk.resolved = true;
            }
            if let Some(next) = proposal.hunks.iter().position(|hunk| !hunk.resolved) {
                proposal.active_hunk = next;
            }
            proposal.hunks.iter().all(|hunk| hunk.resolved)
        } else {
            false
        };
        if complete {
            self.accept_proposal();
        } else if let Some(proposal) = &self.proposal {
            let remaining = proposal.hunks.iter().filter(|hunk| !hunk.resolved).count();
            self.notify(
                Level::Info,
                format!("Change resolved · {remaining} remaining"),
            );
        }
    }

    fn keep_all_changes(&mut self) {
        if self.review.is_some() {
            self.resolve_applied_changes(true, true);
            return;
        }
        if let Some(proposal) = &mut self.proposal {
            for hunk in &mut proposal.hunks {
                hunk.included = true;
                hunk.resolved = true;
            }
        }
        self.accept_proposal();
    }

    fn reject_proposal(&mut self) {
        if self.review.is_some() {
            self.resolve_applied_changes(false, true);
            return;
        }
        self.proposal = None;
        self.follow_editor = true;
        self.notify(
            Level::Info,
            "All agent changes rejected; document unchanged",
        );
    }

    fn discard_proposal_for_manual_edit(&mut self) {
        if self.proposal.take().is_some() {
            self.notify(
                Level::Warning,
                "Agent change review closed because the document was edited; your edit was preserved",
            );
        }
    }

    pub fn sync_review(&mut self) {
        if let Some(review) = &mut self.review {
            let pending = review.pending();
            review.sync(&self.document.text);
            if review.pending() < pending {
                self.notify(Level::Warning,
                    "Manually edited agent changes protected from review Undo; other changes remain reviewable");
            }
        }
    }

    fn resolve_applied_changes(&mut self, keep: bool, all: bool) {
        self.sync_review();
        let Some(review) = &self.review else {
            return;
        };
        let indices = review
            .hunks
            .iter()
            .enumerate()
            .filter(|(i, h)| !h.resolved && (all || *i == review.active))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if indices.is_empty() {
            if review.pending() == 0 {
                self.review = None;
            }
            self.notify(
                Level::Info,
                "No pending changes here; manual edits are preserved",
            );
            return;
        }
        if !keep {
            let mut text = self.document.text.clone();
            for &index in indices.iter().rev() {
                let h = &review.hunks[index];
                if text.get(h.range.clone()) != Some(h.replacement.as_str()) {
                    self.notify(Level::Error, "Change no longer matches; Undo refused");
                    return;
                }
                text.replace_range(h.range.clone(), &h.original);
            }
            let len = self.document.text.len();
            if let Err(error) = self.document.replace_and_save(0..len, &text) {
                self.notify(
                    Level::Error,
                    format!("Undo not applied; could not save: {error:#}"),
                );
                return;
            }
        }
        let review = self.review.as_mut().unwrap();
        for index in indices {
            review.hunks[index].resolved = true;
        }
        // Rebase other edits after the reverted ranges; resolved changes cannot be undone again.
        review.sync(&self.document.text);
        review.advance();
        let remaining = review.pending();
        if remaining == 0 {
            self.review = None;
        } else if let Some(h) = review.hunks.get(review.active) {
            self.document.move_to(h.range.start, false);
        }
        self.follow_editor = true;
        self.notify(
            Level::Success,
            format!(
                "Changes {} · {remaining} pending · already saved{}",
                if keep { "kept" } else { "undone" },
                if self.document.is_temporary() {
                    "; private temporary file; Ctrl+S chooses a filename"
                } else if keep && self.document.dirty() {
                    "; manual edits still unsaved"
                } else {
                    ""
                }
            ),
        );
    }

    fn save(&mut self, path: Option<&Path>) {
        if let Err(e) = self.request_save(path) {
            self.notify(Level::Error, format!("{e:#}"));
        }
    }

    fn request_save(&mut self, path: Option<&Path>) -> Result<()> {
        if path.is_none() && self.document.path.is_none() {
            self.save_prompt = Some(SavePrompt {
                input: String::new(),
                cursor: 0,
                error: None,
            });
            self.notify(Level::Info, "Enter a filename to save; Esc cancels");
        } else {
            self.document.save(path)?;
            self.notify(Level::Success, "Saved");
        }
        Ok(())
    }

    fn save_prompt_insert(&mut self, text: &str) {
        let prompt = self.save_prompt.as_mut().unwrap();
        let text = text.chars().filter(|c| !c.is_control()).collect::<String>();
        prompt.input.insert_str(prompt.cursor, &text);
        let end = prompt.cursor + text.len();
        prompt.cursor = prompt
            .input
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= end)
            .unwrap_or(prompt.input.len());
        prompt.error = None;
    }

    fn save_prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.save_prompt = None;
                self.notify(Level::Info, "Save cancelled");
            }
            KeyCode::Enter => {
                let input = self.save_prompt.as_ref().unwrap().input.clone();
                let result = if input.trim().is_empty() {
                    Err(anyhow::anyhow!("Enter a filename"))
                } else {
                    self.document.save(Some(&self.resolve(&input)))
                };
                match result {
                    Ok(()) => {
                        self.save_prompt = None;
                        self.notify(Level::Success, "Saved");
                        if let Some(operation) = self.pending.take() {
                            self.perform(operation);
                        }
                    }
                    Err(error) => {
                        let error = format!("{error:#}");
                        self.save_prompt.as_mut().unwrap().error = Some(error.clone());
                        self.notify(Level::Error, error);
                    }
                }
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.save_prompt_insert(&c.to_string());
            }
            _ => {
                let prompt = self.save_prompt.as_mut().unwrap();
                match key.code {
                    KeyCode::Left => prompt.cursor = previous(&prompt.input, prompt.cursor),
                    KeyCode::Right => prompt.cursor = next(&prompt.input, prompt.cursor),
                    KeyCode::Home => prompt.cursor = 0,
                    KeyCode::End => prompt.cursor = prompt.input.len(),
                    KeyCode::Backspace if prompt.cursor > 0 => {
                        let start = previous(&prompt.input, prompt.cursor);
                        prompt.input.replace_range(start..prompt.cursor, "");
                        prompt.cursor = start;
                        prompt.error = None;
                    }
                    KeyCode::Delete if prompt.cursor < prompt.input.len() => {
                        let end = next(&prompt.input, prompt.cursor);
                        prompt.input.replace_range(prompt.cursor..end, "");
                        prompt.error = None;
                    }
                    _ => {}
                }
            }
        }
    }
    pub(crate) fn request(&mut self, operation: Pending) {
        if self.document.dirty() || self.proposal.is_some() || self.review.is_some() || self.busy()
        {
            self.pending = Some(operation);
            self.popup_selected = 0;
        } else {
            self.perform(operation);
        }
    }
    fn confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => self.pending = None,
            KeyCode::Up => self.popup_selected = self.popup_selected.saturating_sub(1),
            KeyCode::Down => self.popup_selected = (self.popup_selected + 1).min(2),
            KeyCode::Enter => self.confirm_choice(self.popup_selected),
            KeyCode::Char('d') => self.confirm_choice(1),
            KeyCode::Char('s') => self.confirm_choice(0),
            _ => {}
        }
    }

    fn confirm_choice(&mut self, choice: usize) {
        match choice {
            0 => {
                if self.document.path.is_none() {
                    self.save(None);
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
            1 => {
                if let Some(p) = self.pending.take() {
                    self.perform(p);
                }
            }
            _ => self.pending = None,
        }
    }
    fn perform(&mut self, operation: Pending) {
        self.cancel_live();
        match operation {
            Pending::Quit => {
                self.review = None;
                self.quit = true;
            }
            Pending::Open(path) => match open_or_new(&path) {
                Ok(d) => {
                    self.document = d;
                    self.review = None;
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
            Pending::Recover(id) => {
                let result = self
                    .live
                    .store
                    .as_ref()
                    .context("Recovery storage unavailable")
                    .and_then(|s| s.recover(&id))
                    .and_then(Document::from_recovery);
                match result {
                    Ok(d) => {
                        self.document = d;
                        self.review = None;
                        self.proposal = None;
                        self.attachment = None;
                        self.editor_scroll = 0;
                        self.editor_horizontal = 0;
                        self.follow_editor = true;
                        self.focus = Focus::Editor;
                        self.notify(
                            Level::Success,
                            "Recovery restored into editor only; review and save explicitly",
                        );
                    }
                    Err(e) => self.notify(Level::Error, format!("Recovery failed: {e:#}")),
                }
            }
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
        if self.save_prompt.is_some() {
            return;
        }
        if self.connect.is_some() {
            self.connect_mouse(event);
            return;
        }
        if self.pending.is_some() {
            if let Some(choice) = self.popup_mouse_choice(event) {
                self.popup_selected = choice.min(2);
                self.confirm_choice(self.popup_selected);
            }
            return;
        }
        if self.live.permission.is_some() {
            if let Some(choice) = self.popup_mouse_choice(event) {
                self.popup_selected = choice.min(3);
                self.resolve_permission_choice(self.popup_selected);
            }
            return;
        }
        if self.model_picker.is_some() {
            if let Some(choice) = self.popup_mouse_choice(event) {
                self.select_model_picker_at(choice);
            }
            return;
        }
        if self.agent_picker.is_some() {
            if let Some(choice) = self.popup_mouse_choice(event) {
                self.select_agent_picker_at(choice);
            }
            return;
        }
        if self.recovery_picker.is_some() {
            if let Some(choice) = self.popup_mouse_choice(event) {
                self.select_recovery_picker_at(choice);
            }
            return;
        }
        if self.focus == Focus::Chat
            && !self.completions().is_empty()
            && let Some(choice) = self.popup_mouse_choice(event)
        {
            self.completion = choice.min(self.completions().len() - 1);
            self.complete();
            return;
        }
        let x = event.column;
        let y = event.row;
        if self.explorer.is_none()
            && self.chat_area.contains((x, y).into())
            && matches!(
                event.kind,
                MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
            )
        {
            let down = matches!(event.kind, MouseEventKind::Down(_));
            self.focus = Focus::Chat;
            if self.composer_area.contains((x, y).into())
                && (down || matches!(self.chat_drag, Some(ChatDrag::Composer)))
            {
                if let Some(row) = self
                    .composer_view_rows
                    .get(usize::from(y - self.composer_area.y))
                    .copied()
                {
                    let position = composer_position_at_display(
                        &self.composer,
                        row,
                        usize::from(x - self.composer_area.x),
                    );
                    if down {
                        self.message_selection = None;
                        self.composer_anchor = if event.modifiers.contains(KeyModifiers::SHIFT) {
                            Some(self.composer_anchor.unwrap_or(self.composer_cursor))
                        } else {
                            Some(position)
                        };
                        self.chat_drag = Some(ChatDrag::Composer);
                    }
                    self.composer_cursor = position;
                }
                return;
            }
            if self.chat_history_area.contains((x, y).into()) {
                let hit = self
                    .chat_view_rows
                    .get(usize::from(y - self.chat_history_area.y))
                    .copied()
                    .flatten();
                if let Some(hit) = hit
                    && (down
                        || matches!(self.chat_drag, Some(ChatDrag::Message(index)) if index == hit.message))
                    && let Some(message) = self.messages.get(hit.message)
                {
                    let text = display_text(&message.text);
                    let position = composer_position_at_display(
                        &text,
                        hit.row,
                        usize::from(x - self.chat_history_area.x).saturating_sub(hit.left),
                    );
                    if down {
                        self.composer_anchor = None;
                        self.message_selection = Some(MessageSelection {
                            message: hit.message,
                            anchor: position,
                            cursor: position,
                        });
                        self.chat_drag = Some(ChatDrag::Message(hit.message));
                        self.follow_chat = false;
                    } else if let Some(selection) = &mut self.message_selection {
                        selection.cursor = position;
                    }
                    return;
                }
            }
            if down {
                self.message_selection = None;
                self.composer_anchor = None;
                self.chat_drag = None;
            }
        }
        if self.editor_area.contains((x, y).into()) {
            if event.kind == MouseEventKind::Down(MouseButton::Left) {
                self.chat_drag = None;
                self.sync_review();
                if let Some((_, index, keep)) = self
                    .review_buttons
                    .iter()
                    .find(|(rect, _, _)| rect.contains((x, y).into()))
                    .copied()
                    && let Some(review) = &mut self.review
                    && review.hunks.get(index).is_some_and(|h| !h.resolved)
                {
                    review.active = index;
                    self.focus = Focus::Editor;
                    self.dragging = false;
                    self.resolve_applied_changes(keep, false);
                    // Until the next render, these screen coordinates are stale.
                    self.review_buttons.clear();
                    self.review_row_hunks.clear();
                    self.review_view_rows.clear();
                    return;
                }
            }
            match event.kind {
                MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left) => {
                    self.focus = Focus::Editor;
                    let visible = usize::from(y - self.editor_area.y);
                    if let Some(Some(index)) = self.review_row_hunks.get(visible)
                        && let Some(review) = &mut self.review
                        && review.hunks.get(*index).is_some_and(|h| !h.resolved)
                    {
                        review.active = *index;
                    }
                    let Some((row, start)) = self.editor_view_rows.get(visible).copied() else {
                        return;
                    };
                    let (row, start) = if self.review.is_some() {
                        let Some(Some(position)) = self.review_view_rows.get(visible) else {
                            return;
                        };
                        *position
                    } else {
                        (row, start)
                    };
                    let col = start
                        + usize::from(x - self.editor_area.x).saturating_sub(self.review_gutter);
                    let p = self.document.position_at_display(row, col);
                    self.document.move_to(
                        p,
                        matches!(event.kind, MouseEventKind::Drag(_)) && self.dragging,
                    );
                    self.select_change_at_cursor();
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
            self.chat_drag = None;
        }
    }

    fn connect_mouse(&mut self, event: MouseEvent) {
        if matches!(
            self.connect.as_ref().map(|wizard| wizard.step),
            Some(ConnectStep::PlaintextConfirm)
        ) {
            if let Some(choice) = self.popup_mouse_choice(event) {
                self.popup_selected = choice.min(1);
                if let Some(wizard) = self.connect.as_mut() {
                    wizard.step = if self.popup_selected == 0 {
                        ConnectStep::SecretFile
                    } else {
                        ConnectStep::Credential
                    };
                }
            }
            return;
        }
        if !matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
            || event.column < self.connect_choice_area.x
            || event.column >= self.connect_choice_area.right()
            || event.row < self.connect_choice_area.y
            || event.row >= self.connect_choice_area.bottom()
        {
            return;
        }
        let selected = usize::from(event.row - self.connect_choice_area.y);
        match self.connect.as_ref().map(|wizard| wizard.step) {
            Some(ConnectStep::Provider) => self.select_connect_provider_at(selected),
            Some(ConnectStep::Credential) => self.select_connect_credential_at(selected),
            _ => {}
        }
    }

    fn popup_mouse_choice(&self, event: MouseEvent) -> Option<usize> {
        if !matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
            || !self
                .popup_choice_area
                .contains((event.column, event.row).into())
        {
            return None;
        }
        Some(self.popup_choice_start + usize::from(event.row - self.popup_choice_area.y))
    }

    fn select_model_picker_at(&mut self, selected: usize) {
        let Some(picker) = self.model_picker.as_ref() else {
            return;
        };
        let Some(model) = picker.models.get(selected).cloned() else {
            return;
        };
        self.model_picker = None;
        if let Err(error) = self.select_model(model) {
            self.notify(Level::Error, format!("Could not remember model: {error:#}"));
        }
    }

    fn load_agents(&mut self) -> Result<()> {
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
                let agent = Agent::parse_markdown(&fs::read_to_string(&path)?)?;
                if agent.name.trim().is_empty()
                    || self
                        .agents
                        .iter()
                        .any(|a| a.name.eq_ignore_ascii_case(&agent.name))
                {
                    bail!("Empty or duplicate agent name");
                }
                Ok(agent)
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

impl AgentMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "CHAT",
            Self::Agent => "AGENT",
        }
    }
}

impl Proposal {
    pub fn new(
        mock: bool,
        revision: u64,
        range: Range<usize>,
        original: String,
        replacement: String,
    ) -> Self {
        let diff = similar::TextDiff::from_lines(&original, &replacement);
        let old_offsets = line_offsets(&original);
        let new_offsets = line_offsets(&replacement);
        let hunks = diff
            .grouped_ops(3)
            .into_iter()
            .filter_map(|operations| {
                let first = operations.first()?;
                let last = operations.last()?;
                let old = first.old_range().start..last.old_range().end;
                let new = first.new_range().start..last.new_range().end;
                let old_range = old_offsets[old.start]..old_offsets[old.end];
                let new_range = new_offsets[new.start]..new_offsets[new.end];
                Some(ProposalHunk {
                    original: original[old_range.clone()].into(),
                    replacement: replacement[new_range].into(),
                    range: old_range,
                    included: true,
                    resolved: false,
                })
            })
            .collect();
        Self {
            mock,
            revision,
            range,
            original,
            replacement,
            hunks,
            active_hunk: 0,
        }
    }

    pub fn included_replacement(&self) -> String {
        let mut output = String::new();
        let mut cursor = 0;
        for hunk in &self.hunks {
            output.push_str(&self.original[cursor..hunk.range.start]);
            if hunk.included {
                output.push_str(&hunk.replacement);
            } else {
                output.push_str(&self.original[hunk.range.clone()]);
            }
            cursor = hunk.range.end;
        }
        output.push_str(&self.original[cursor..]);
        output
    }
}

fn line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for line in text.split_inclusive('\n') {
        offsets.push(offsets.last().copied().unwrap_or(0) + line.len());
    }
    offsets
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

#[derive(Clone, Copy)]
pub(crate) struct ComposerRow {
    pub start: usize,
    pub end: usize,
}

pub(crate) fn composer_rows(text: &str, width: usize) -> Vec<ComposerRow> {
    word_wrapped_rows(text, width, false)
}

/// Source-preserving visual rows. Editor tabs use logical-line columns; prompt
/// and message tabs use visual-row columns. Trailing separators stay attached
/// to the preceding row, even when their invisible cells are clipped.
pub(crate) fn word_wrapped_rows(text: &str, width: usize, logical_tabs: bool) -> Vec<ComposerRow> {
    use unicode_segmentation::UnicodeSegmentation;

    let width = width.max(1);
    let mut rows = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        let graphemes = line.grapheme_indices(true).collect::<Vec<_>>();
        // Cache remaining word widths to avoid rescanning long unbroken text.
        let mut words = vec![0; graphemes.len() + 1];
        for (index, (_, grapheme)) in graphemes.iter().enumerate().rev() {
            if !grapheme.chars().all(char::is_whitespace) {
                words[index] = grapheme_width(grapheme, 0) + words[index + 1];
            }
        }
        let mut start = 0;
        let mut column = 0;
        let mut logical_column = 0;
        let mut has_word = false;
        for (index, &(byte, grapheme)) in graphemes.iter().enumerate() {
            let whitespace = grapheme.chars().all(char::is_whitespace);
            let word_start = !whitespace
                && (index == 0 || graphemes[index - 1].1.chars().all(char::is_whitespace));
            let mut cells =
                grapheme_width(grapheme, if logical_tabs { logical_column } else { column });
            let move_word = word_start && words[index] <= width && column + words[index] > width;
            let overflow = column + cells > width && !(whitespace && has_word);
            if byte > start && (move_word || overflow) {
                rows.push(ComposerRow {
                    start: offset + start,
                    end: offset + byte,
                });
                start = byte;
                column = 0;
                has_word = false;
                if !logical_tabs {
                    cells = grapheme_width(grapheme, 0);
                }
            }
            column += cells;
            logical_column += cells;
            has_word |= !whitespace;
        }
        rows.push(ComposerRow {
            start: offset + start,
            end: offset + line.len(),
        });
        offset += line.len() + 1;
    }
    rows
}

pub(crate) fn composer_display_width(text: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation;

    text.graphemes(true).fold(0, |column, grapheme| {
        column + grapheme_width(grapheme, column)
    })
}

fn composer_position_at_display(text: &str, row: ComposerRow, target: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;

    let mut column = 0;
    let Some(content) = text.get(row.start..row.end) else {
        // A queued mouse event may still carry coordinates from before an edit.
        return text.len();
    };
    for (byte, grapheme) in content.grapheme_indices(true) {
        let width = grapheme_width(grapheme, column);
        if target < column + width {
            return row.start + byte;
        }
        column += width;
    }
    row.end
}

/// Sanitize terminal controls consistently for reply rendering, selection and copy.
pub(crate) fn display_text(text: &str) -> String {
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
    #[test]
    fn embedded_default_agents_preserve_order_descriptions_and_prompts() {
        let agents = Agent::defaults().unwrap();
        assert!(agents.len() >= 2);
        assert_eq!(agents[0].name, "Planner");
        assert_eq!(agents[0].description, "Specifications and plans");
        assert_eq!(
            agents[0].instructions,
            "Clarify intent, assumptions, risks, and acceptance criteria."
        );
        assert_eq!(agents[1].name, "Blogger");
        assert_eq!(agents[1].description, "Blog drafts and revisions");
        assert_eq!(
            agents[1].instructions,
            "Preserve voice, audience, and meaning. Flag claims needing verification."
        );
    }

    #[test]
    fn default_agent_files_accept_additional_agents_and_markdown_prompts() {
        let agents = Agent::parse_defaults(&[
            ("01-researcher.md", "---\nname: Researcher\ndescription: Research notes\n---\n\n# Instructions\n\n- Check sources.\n- Preserve citations.\n"),
            ("02-reviewer.md", "---\nname: Reviewer\ndescription: Review drafts\n---\nReview clarity.\n"),
            ("03-translator.md", "---\nname: Translator\n---\nPreserve meaning.\n"),
        ]).unwrap();
        assert_eq!(agents.len(), 3);
        assert_eq!(
            agents[0].instructions,
            "# Instructions\n\n- Check sources.\n- Preserve citations."
        );
        assert_eq!(agents[2].name, "Translator");
    }

    #[test]
    fn default_agent_files_reject_empty_invalid_and_duplicate_definitions() {
        assert!(Agent::parse_defaults(&[]).is_err());
        for source in [
            "No frontmatter",
            "---\nname: Planner\n",                  // Missing closing marker.
            "---\ndescription: Plans\n---\nPlan.\n", // Missing name.
            "---\nname: ''\n---\nPlan.\n",
            "---\nname: Planner\n---\n   \n", // Empty prompt.
            "---\nname: [Planner]\n---\nPlan.\n",
        ] {
            assert!(
                Agent::parse_defaults(&[("bad.md", source)]).is_err(),
                "{source}"
            );
        }
        assert!(
            Agent::parse_defaults(&[
                ("one.md", "---\nname: Planner\n---\nPlan.\n"),
                ("two.md", "---\nname: planner\n---\nPlan again.\n"),
            ])
            .is_err()
        );
    }

    #[test]
    fn agent_markdown_parser_preserves_body_and_supports_crlf_and_optional_description() {
        let agent = Agent::parse_markdown(
            "---\r\nname: Reviewer\r\n---\r\n\r\n# Review\r\n\r\n**Preserve** voice.\r\n",
        )
        .unwrap();
        assert_eq!(agent.name, "Reviewer");
        assert!(agent.description.is_empty());
        assert_eq!(agent.instructions, "# Review\n\n**Preserve** voice.");
    }

    #[test]
    fn build_embeds_every_agent_markdown_file_in_filename_order() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/agents");
        let mut filenames = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| {
                entry.file_type().unwrap().is_file()
                    && entry.path().extension().is_some_and(|ext| ext == "md")
            })
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        filenames.sort();
        assert_eq!(
            DEFAULT_AGENT_SOURCES
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            filenames
        );
        for &(filename, content) in DEFAULT_AGENT_SOURCES {
            assert_eq!(
                content,
                fs::read_to_string(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("assets/agents")
                        .join(filename)
                )
                .unwrap()
            );
        }
    }
    fn app() -> App {
        App::new(None).unwrap()
    }
    #[test]
    fn agent_picker_supports_keyboard_mouse_cancel_and_direct_selection() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.agent = 0;
        a.compose_insert("/agents");
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(a.agent_picker, Some(0));
        assert!(a.composer.is_empty());
        a.paste("ignored");
        assert!(a.composer.is_empty());
        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(a.agent_picker, Some(0));
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(a.agent, 1);
        assert_eq!(a.config.agent, a.agents[1].name);
        assert!(a.agent_picker.is_none());

        a.command("/agents").unwrap();
        assert_eq!(a.agent_picker, Some(1));
        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(a.agent, 1);
        assert!(a.agent_picker.is_none());

        a.command("/agents").unwrap();
        a.popup_choice_area = Rect::new(10, 5, 40, 1);
        a.popup_choice_start = 0;
        a.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(a.agent, 0);
        assert!(a.agent_picker.is_none());
        a.command("/agents Blogger").unwrap();
        assert_eq!(a.agents[a.agent].name, "Blogger");
        assert!(a.agent_picker.is_none());
    }

    #[test]
    fn prompt_control_line_keys_preserve_unicode_newlines_and_document() {
        for profile in ["vscode", "emacs"] {
            let mut a = app();
            a.config.keybindings = profile.into();
            a.focus = Focus::Chat;
            a.composer_area = Rect::new(0, 0, 4, 3);
            a.compose_insert("first\n界e\u{301}👩‍💻 tail\nlast");
            a.composer_cursor = "first\n界e\u{301}".len();
            a.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
            assert_eq!(a.composer_cursor, "first\n".len());
            a.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
            assert_eq!(a.composer_cursor, "first\n界e\u{301}👩‍💻 tail".len());
            let original = a.composer.clone();
            a.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
            assert_eq!(a.composer, original);
            a.composer_cursor = "first\n界e\u{301}".len();
            a.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
            assert_eq!(a.composer, "first\n界e\u{301}\nlast");
            assert_eq!(a.composer_cursor, "first\n界e\u{301}".len());
            a.composer_cursor = a.composer.len();
            a.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
            a.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
            assert_eq!(a.composer, "first\n界e\u{301}\n");
            assert!(a.document.text.is_empty());
            assert!(a.messages.iter().all(|message| message.role != "you"));
        }
    }

    #[test]
    fn killing_recalled_prompt_exits_history_without_modifying_saved_prompt() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.compose_insert("original prompt");
        a.submit();
        a.previous_prompt();
        a.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        a.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert!(a.composer.is_empty());
        assert!(a.prompt_history_index.is_none());
        assert_eq!(a.prompt_history.last().unwrap(), "original prompt");
    }

    #[test]
    fn version_command_completes_and_displays_package_version_without_editing() {
        let mut a = app();
        a.document.insert("界 draft");
        let revision = a.document.revision;
        a.focus = Focus::Chat;
        a.compose_insert("/ver");
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let message = a.messages.last().unwrap();
        assert_eq!(message.role, "version");
        assert_eq!(
            message.text,
            format!("queditor {}", env!("CARGO_PKG_VERSION"))
        );
        assert!(a.messages.iter().all(|message| message.role != "you"));
        assert!(a.composer.is_empty());
        assert_eq!(a.prompt_history.last().unwrap(), "/version");
        assert_eq!(a.document.text, "界 draft");
        assert_eq!(a.document.revision, revision);
        a.command("/help").unwrap();
        assert!(a.messages.last().unwrap().text.contains("/version"));
    }

    #[test]
    fn slash_commands_do_not_echo_but_output_and_prompts_remain_visible() {
        for mode in [AgentMode::Chat, AgentMode::Agent] {
            let mut a = app();
            a.config
                .agent_modes
                .insert(a.agents[a.agent].name.clone(), mode);
            a.compose_insert("/help");
            a.submit();
            assert!(!a.messages.iter().any(|message| message.role == "you"));
            assert!(
                a.messages
                    .iter()
                    .any(|message| message.text.contains("/help"))
            );
            assert_eq!(a.prompt_history.last().unwrap(), "/help");
            a.compose_insert("Hello");
            a.submit();
            assert!(
                a.messages
                    .iter()
                    .any(|message| message.role == "you" && message.text == "Hello")
            );
        }
    }

    #[test]
    fn enter_runs_highlighted_slash_completion() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.compose_insert("/co");
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(a.connect.is_some());
        assert!(a.composer.is_empty());
        assert!(!a.messages.iter().any(|message| message.role == "you"));
        assert_eq!(a.prompt_history.last().unwrap(), "/connect");
    }

    #[test]
    fn enter_runs_arrow_selected_command_and_preserves_failed_input() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.compose_insert("/c");
        while a.completions()[a.completion] != "/cd" {
            a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(a.level, Level::Error));
        assert_eq!(a.status, "Usage: /cd <path>");
        assert_eq!(a.composer.trim(), "/cd");
        assert!(!a.messages.iter().any(|message| message.role == "you"));
    }

    #[test]
    fn tab_completes_without_running_and_modified_enter_inserts_newline() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.compose_insert("/co");
        a.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(a.composer, "/connect ");
        assert!(a.connect.is_none());
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        assert_eq!(a.composer, "/connect \n");
        assert!(a.connect.is_none());
    }

    #[test]
    fn enter_submits_exact_commands_and_arguments_unchanged() {
        for input in ["/find", "/theme unknown", "/unknown", "/save \"unclosed"] {
            let mut a = app();
            a.focus = Focus::Chat;
            a.compose_insert(input);
            a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert!(matches!(a.level, Level::Error));
            assert_eq!(a.composer.trim(), input);
            assert!(!a.messages.iter().any(|message| message.role == "you"));
        }
    }

    #[test]
    fn unnamed_save_commands_and_shortcut_prompt_for_filename() {
        for input in ["/sa", "/save", ""] {
            let mut a = app();
            a.document.insert("unsaved");
            if input.is_empty() {
                a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
            } else {
                a.focus = Focus::Chat;
                a.compose_insert(input);
                a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            assert!(a.save_prompt.is_some());
            assert!(a.document.path.is_none());
            a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(a.save_prompt.is_none());
            assert_eq!(a.document.text, "unsaved");
            assert!(a.document.dirty());
        }
    }

    #[test]
    fn filename_popup_saves_accepted_text_and_named_shortcut_skips_popup() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = app();
        a.workspace = dir.path().to_owned();
        a.document.insert("hello");
        a.command("/demo").unwrap();
        a.command("/save").unwrap();
        a.paste("draft 界 name.md");
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(a.save_prompt.is_none());
        assert_eq!(
            fs::read_to_string(dir.path().join("draft 界 name.md")).unwrap(),
            "hello"
        );
        assert!(a.proposal.is_some());
        assert!(!a.document.dirty());
        a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(a.save_prompt.is_none());
    }

    #[test]
    fn filename_popup_retries_empty_missing_parent_and_existing_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("existing.md"), "original").unwrap();
        let mut a = app();
        a.workspace = dir.path().to_owned();
        a.document.insert("unsaved");
        a.command("/save").unwrap();
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            a.save_prompt.as_ref().unwrap().error.as_deref(),
            Some("Enter a filename")
        );
        for path in ["missing/draft.md", "existing.md"] {
            a.save_prompt.as_mut().unwrap().input.clear();
            a.save_prompt.as_mut().unwrap().cursor = 0;
            a.paste(path);
            a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert!(a.save_prompt.as_ref().unwrap().error.is_some());
            assert!(a.document.path.is_none());
            assert!(a.document.dirty());
            assert_eq!(a.save_prompt.as_ref().unwrap().input, path);
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("existing.md")).unwrap(),
            "original"
        );
    }

    #[test]
    fn filename_popup_is_unicode_editable_and_paste_never_submits() {
        let mut a = app();
        a.document.insert("unchanged");
        a.compose_insert("draft prompt");
        a.command("/save").unwrap();
        a.paste("界e\u{301}\n.md\r");
        a.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(a.save_prompt.as_ref().unwrap().input, "界.md");
        a.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(a.save_prompt.as_ref().unwrap().input, ".md");
        assert_eq!(a.document.text, "unchanged");
        assert_eq!(a.composer, "draft prompt");
        assert!(a.document.path.is_none());
    }

    #[test]
    fn unnamed_save_and_continue_resumes_pending_operation() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = app();
        a.workspace = dir.path().to_owned();
        a.document.insert("unsaved");
        a.request(Pending::Quit);
        a.confirm_choice(0);
        assert!(a.save_prompt.is_some());
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.pending.is_some());
        assert!(!a.quit);
        a.confirm_choice(0);
        a.paste("saved.md");
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(a.quit);
        assert!(a.pending.is_none());
        assert_eq!(
            fs::read_to_string(dir.path().join("saved.md")).unwrap(),
            "unsaved"
        );
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
    fn individual_diff_blocks_apply_atomically() {
        let mut a = app();
        let original = "one\nkeep a\nkeep b\nkeep c\nkeep d\nkeep e\nkeep f\nkeep g\nkeep h\nten\n";
        let replacement =
            "ONE\nkeep a\nkeep b\nkeep c\nkeep d\nkeep e\nkeep f\nkeep g\nkeep h\nTEN\n";
        a.document.insert(original);
        a.proposal = Some(Proposal::new(
            false,
            a.document.revision,
            0..original.len(),
            original.into(),
            replacement.into(),
        ));
        assert_eq!(a.proposal.as_ref().unwrap().hunks.len(), 2);
        a.proposal.as_mut().unwrap().hunks[1].included = false;
        a.accept_proposal();
        assert_eq!(
            a.document.text,
            "ONE\nkeep a\nkeep b\nkeep c\nkeep d\nkeep e\nkeep f\nkeep g\nkeep h\nten\n"
        );
        a.document.undo();
        assert_eq!(a.document.text, original);
    }
    #[test]
    fn dedicated_change_keys_resolve_without_modal_review() {
        let mut a = app();
        a.document.insert("hello");
        a.proposal = Some(Proposal::new(
            false,
            a.document.revision,
            0..5,
            "hello".into(),
            "HELLO".into(),
        ));
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(a.document.text, "HELLO");
        assert!(a.proposal.is_none());
        a.document.undo();
        assert_eq!(a.document.text, "hello");
    }
    #[test]
    fn manual_edit_preserves_user_work_and_closes_review() {
        let mut a = app();
        a.document.insert("hello");
        a.proposal = Some(Proposal::new(
            false,
            a.document.revision,
            0..5,
            "hello".into(),
            "HELLO".into(),
        ));
        a.key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
        assert_eq!(a.document.text, "hello!");
        assert!(a.proposal.is_none());
    }
    #[test]
    fn proposals_support_empty_documents_and_complete_deletion() {
        let insertion = Proposal::new(false, 0, 0..0, String::new(), "hello\n".into());
        assert_eq!(insertion.hunks.len(), 1);
        assert_eq!(insertion.included_replacement(), "hello\n");

        let deletion = Proposal::new(false, 0, 0..6, "hello\n".into(), String::new());
        assert_eq!(deletion.hunks.len(), 1);
        assert_eq!(deletion.included_replacement(), "");
    }
    #[test]
    fn agent_modes_are_independent_per_agent() {
        let mut a = app();
        a.config.agent_modes.clear();
        assert_eq!(a.agent_mode(), AgentMode::Chat);
        a.config
            .agent_modes
            .insert("Planner".into(), AgentMode::Agent);
        assert_eq!(a.agent_mode(), AgentMode::Agent);
        a.agent = 1;
        assert_eq!(a.agent_mode(), AgentMode::Chat);
    }

    #[test]
    fn shift_tab_rotates_every_builtin_pair_from_both_panes_and_encodings() {
        for focus in [Focus::Editor, Focus::Chat] {
            for key in [
                KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
                KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT),
            ] {
                let mut a = app();
                a.agents.truncate(2);
                a.agent = 1; // Blogger Chat, regardless of saved preferences.
                a.config.agent = "Blogger".into();
                a.config.agent_modes.clear();
                // Rotation must not skip Planner Chat because Planner remembers Agent.
                a.config
                    .agent_modes
                    .insert("Planner".into(), AgentMode::Agent);
                a.focus = focus;
                a.document.insert("界 draft");
                a.compose_insert("/ag"); // Must not trigger Tab completion or submission.
                let revision = a.document.revision;
                for (agent, mode) in [
                    ("Blogger", AgentMode::Agent),
                    ("Planner", AgentMode::Chat),
                    ("Planner", AgentMode::Agent),
                    ("Blogger", AgentMode::Chat),
                ] {
                    a.key(key);
                    assert_eq!(a.agents[a.agent].name, agent);
                    assert_eq!(a.config.agent, agent);
                    assert_eq!(a.agent_mode(), mode);
                    assert!(a.focus == focus);
                    assert_eq!(a.composer, "/ag");
                    assert_eq!(a.document.text, "界 draft");
                    assert_eq!(a.document.revision, revision);
                }
            }
        }
    }

    #[test]
    fn rotation_includes_custom_agents_and_single_agent_wraps_modes() {
        let mut a = app();
        a.agents.truncate(2);
        for name in ["Reviewer", "研究者"] {
            a.agents.push(Agent {
                name: name.into(),
                description: String::new(),
                instructions: "Custom instructions".into(),
            });
        }
        a.agent = 0;
        a.config.agent_modes.clear();
        let pairs = a
            .agents
            .iter()
            .enumerate()
            .flat_map(|(index, _)| AgentMode::ALL.iter().map(move |mode| (index, *mode)))
            .collect::<Vec<_>>();
        for &(index, mode) in pairs.iter().skip(1).chain(pairs.iter().take(1)) {
            a.rotate_agent_mode().unwrap();
            assert_eq!((a.agent, a.agent_mode()), (index, mode));
        }
        a.agents.truncate(1);
        for &mode in AgentMode::ALL
            .iter()
            .skip(1)
            .chain(AgentMode::ALL.iter().take(1))
        {
            a.rotate_agent_mode().unwrap();
            assert_eq!((a.agent, a.agent_mode()), (0, mode));
        }
    }

    #[test]
    fn control_g_uses_shared_mode_order_without_switching_agents() {
        let mut a = app();
        a.agent = 1;
        a.config.agent_modes.clear();
        for &mode in AgentMode::ALL
            .iter()
            .skip(1)
            .chain(AgentMode::ALL.iter().take(1))
        {
            a.key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
            assert_eq!((a.agent, a.agent_mode()), (1, mode));
        }
    }

    #[test]
    fn failed_agent_preference_save_preserves_selection_and_modes() {
        let mut a = app();
        let name = a.config.agent.clone();
        let modes = a.config.agent_modes.clone();
        let index = a.agent;
        // A directory at the config-file path prevents atomic replacement on
        // every platform without relying on Unix permission behavior.
        fs::create_dir_all(config_dir().join("config.yaml")).unwrap();
        assert!(a.select_agent_with_mode(1, AgentMode::Agent).is_err());
        assert_eq!(a.agent, index);
        assert_eq!(a.config.agent, name);
        assert_eq!(a.config.agent_modes, modes);
    }

    #[test]
    fn rotation_with_no_agents_returns_error_without_changing_preferences() {
        let mut a = app();
        let name = a.config.agent.clone();
        let modes = a.config.agent_modes.clone();
        a.agents.clear();
        assert!(a.rotate_agent_mode().is_err());
        assert_eq!(a.config.agent, name);
        assert_eq!(a.config.agent_modes, modes);
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
    fn current_selection_becomes_labeled_prompt_attachment() {
        let mut a = app();
        a.document.insert("one\ntwo\nthree");
        a.document.move_to(4, false);
        a.document.move_to(13, true);

        let attachment = a.selected_attachment().unwrap();
        assert_eq!(attachment.text, "two\nthree");
        assert_eq!(attachment.label, "Selected lines 2:3");
        assert_eq!(attachment.revision, a.document.revision);
        assert_eq!(attachment.range, 4..13);
    }
    #[test]
    fn single_line_selection_uses_singular_label() {
        let mut a = app();
        a.document.insert("one\ntwo\nthree");
        a.document.move_to(4, false);
        a.document.move_to(7, true);

        let attachment = a.selected_attachment().unwrap();
        assert_eq!(attachment.text, "two");
        assert_eq!(attachment.label, "Selected line 2");
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
    fn default_control_line_shortcuts_move_and_delete() {
        let mut a = app();
        a.document.insert("one two\nthree");
        a.document.move_to(5, false);
        a.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(a.document.cursor, 0);
        a.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(a.document.cursor, 7);
        a.document.move_to(4, false);
        a.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(a.document.text, "one \nthree");
        a.document.undo();
        assert_eq!(a.document.text, "one two\nthree");
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
    fn prompt_selection_copy_cut_and_paste_are_unicode_safe_in_both_profiles() {
        for (profile, copy, cut, paste) in [
            (
                "vscode",
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                'x',
                'v',
            ),
            (
                "emacs",
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT),
                'w',
                'y',
            ),
        ] {
            let mut a = app();
            a.focus = Focus::Chat;
            a.bindings = Bindings::load(profile).unwrap();
            a.compose_insert("hi 界e\u{301}👩‍💻");
            let before = a.messages.len();
            for _ in 0..3 {
                a.key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
            }
            assert_eq!(a.chat_selected_text().as_deref(), Some("界e\u{301}👩‍💻"));
            a.key(copy);
            assert_eq!(a.clipboard, "界e\u{301}👩‍💻");
            a.key(KeyEvent::new(KeyCode::Char(cut), KeyModifiers::CONTROL));
            assert_eq!(a.composer, "hi ");
            a.key(KeyEvent::new(KeyCode::Char(paste), KeyModifiers::CONTROL));
            assert_eq!(a.composer, "hi 界e\u{301}👩‍💻");
            assert!(a.composer_anchor.is_none());
            assert_eq!(a.messages.len(), before);
            assert!(a.document.text.is_empty());
        }
    }

    #[test]
    fn prompt_select_all_replacement_deletion_and_escape_preserve_unsent_text() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.compose_insert("one\ntwo");
        a.key(KeyEvent::new(
            KeyCode::Char('A'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        a.paste("new\nprompt");
        assert_eq!(a.composer, "new\nprompt");
        assert!(a.composer_anchor.is_none());
        a.key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(a.composer, "new\nprompt");
        a.key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        a.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(a.composer, "new\npromt");
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.composer.is_empty());
    }

    #[test]
    fn shifted_prompt_arrows_select_instead_of_recalling_history() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.config.model = "mock".into();
        a.compose_insert("old prompt");
        a.submit();
        a.compose_insert("one\ntwo");
        a.composer_area = Rect::new(0, 0, 40, 3);
        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
        assert_eq!(a.composer, "one\ntwo");
        assert_eq!(a.chat_selected_text().as_deref(), Some("\ntwo"));
        a.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert!(a.composer_anchor.is_none());
    }
    #[test]
    fn prompt_history_recalls_submissions_and_restores_draft() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.config.model = "mock".into();
        a.composer_area = Rect::new(0, 0, 40, 4);
        for prompt in ["first", "second"] {
            a.compose_insert(prompt);
            a.submit();
        }
        a.compose_insert("unsent draft");

        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(a.composer, "second");
        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(a.composer, "first");
        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(a.composer, "first");
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(a.composer, "second");
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(a.composer, "unsent draft");
    }
    #[test]
    fn prompt_arrows_move_visual_rows_before_recalling_history() {
        let mut a = app();
        a.focus = Focus::Chat;
        a.config.model = "mock".into();
        a.composer_area = Rect::new(0, 0, 4, 4);
        a.compose_insert("old");
        a.submit();
        a.compose_insert("abcdEF");

        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(a.composer, "abcdEF");
        assert_eq!(a.composer_cursor, 2);
        a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(a.composer, "old");

        a.composer = "one\ntwo".into();
        a.composer_cursor = 0;
        a.stop_browsing_prompt_history();
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(a.composer, "one\ntwo");
        assert_eq!(a.composer_cursor, 4);
    }
    #[test]
    fn terminal_paste_populates_masked_api_key_without_touching_chat() {
        let mut a = app();
        let before = a.messages.len();
        a.command("/connect").unwrap();
        a.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        a.paste("  secret-key\r\n");

        let wizard = a.connect.as_ref().unwrap();
        assert_eq!(wizard.secret, "secret-key");
        assert!(wizard.input.is_empty());
        assert!(a.composer.is_empty());
        assert_eq!(a.messages.len(), before);
    }
    #[test]
    fn connect_provider_list_supports_arrows_enter_and_mouse() {
        let mut a = app();
        a.command("/connect").unwrap();
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(a.connect.as_ref().unwrap().provider_selected, 2);
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(a.connect.as_ref().unwrap().provider_id, "gemini");

        a.connect = None;
        a.command("/connect").unwrap();
        a.connect_choice_area = Rect::new(10, 5, 40, CONNECT_PROVIDER_NAMES.len() as u16);
        a.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(a.connect.as_ref().unwrap().provider_id, "openrouter");
    }
    #[test]
    fn connect_credential_list_supports_arrows_enter_and_mouse() {
        let mut a = app();
        a.command("/connect").unwrap();
        a.select_connect_provider_at(0);
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(a.connect.as_ref().unwrap().step, ConnectStep::Env));

        a.connect.as_mut().unwrap().step = ConnectStep::Credential;
        a.connect_choice_area = Rect::new(10, 5, 40, CONNECT_CREDENTIAL_NAMES.len() as u16);
        a.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 7,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(
            a.connect.as_ref().unwrap().step,
            ConnectStep::PlaintextConfirm
        ));
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
    #[test]
    fn confirmation_supports_arrows_enter_and_mouse() {
        let mut a = app();
        a.document.insert("unsaved");
        a.request(Pending::Quit);
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(a.popup_selected, 1);
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(a.pending.is_none());
        assert!(!a.quit);

        a.request(Pending::Quit);
        a.popup_choice_area = Rect::new(10, 5, 40, 3);
        a.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 6,
            modifiers: KeyModifiers::NONE,
        });
        assert!(a.quit);
    }
    #[test]
    fn plaintext_warning_supports_arrows_enter_and_mouse() {
        let mut a = app();
        a.command("/connect").unwrap();
        a.select_connect_provider_at(0);
        a.select_connect_credential_at(2);
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            a.connect.as_ref().unwrap().step,
            ConnectStep::Credential
        ));

        a.select_connect_credential_at(2);
        a.popup_choice_area = Rect::new(10, 5, 40, 2);
        a.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(
            a.connect.as_ref().unwrap().step,
            ConnectStep::SecretFile
        ));
    }
    #[test]
    fn model_picker_supports_arrows_enter_and_mouse() {
        let mut a = app();
        a.config.models = vec!["provider::one".into(), "provider::two".into()];
        a.open_model_picker();
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(a.model_picker.is_none());

        a.open_model_picker();
        let selected = a.model_picker.as_ref().unwrap().models.len() - 1;
        let expected = a.model_picker.as_ref().unwrap().models[selected].clone();
        a.popup_choice_area = Rect::new(10, 5, 40, (selected + 1) as u16);
        a.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 5 + selected as u16,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(a.config.model, expected);
        assert!(a.model_picker.is_none());
    }
}
