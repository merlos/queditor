# Configuration

On Linux/macOS, the app reads `$XDG_CONFIG_HOME/queditor/config.yaml`, or
`~/.config/queditor/config.yaml` when XDG_CONFIG_HOME is unset. On Windows it reads
`%APPDATA%\queditor\config.yaml`. It does not create
or modify home-directory configuration automatically.

```yaml
schema_version: 1
theme: mocha           # mocha or daylight
keybindings: vscode    # vscode, emacs, or a custom profile filename
agent: Planner         # last selected built-in or custom agent
agent_modes:            # remembered chat/agent mode by agent name
  Planner: agent        # pristine-install default
model: mock            # or a genai provider::model identifier
models: [mock]         # suggestions shown by /models
providers: {}          # connections created by /connect
persist_sessions: false
recovery: true
```

Missing fields inherit embedded defaults. Unknown fields, invalid themes, and
unsupported schema versions warn and fall back to safe defaults. A bad binding
profile falls back to vscode. Model, agent, and each agent's Chat/Agent mode are
saved immediately and restored on the next launch; a removed custom agent falls
back to Planner. A pristine environment starts with Planner in Agent mode. After
the user selects a mode, that saved selection is restored as before. Other agents
without a saved mode default to read-only Chat mode.
Live Agent mode automatically applies and saves completed edits before Keep/Undo
review, including existing unsaved text. There is no approval-before-save option.
Unnamed documents use private temporary backing until Ctrl+S/Save As chooses a real
file; subsequent agent edits save that file. `recovery: true` also
requires a durable pre-edit snapshot before auto-save and retains it after Keep/exit;
`recovery: false` explicitly disables that safeguard, not automatic saving.
`/theme daylight` changes the theme for the current run only.
Both themes include a lighter surface for the borderless editor, prompt, and
submitted messages; agent replies use the standard background.

## Providers and credentials

`/connect` writes non-secret provider metadata to `config.yaml`. A custom
OpenAI-compatible connection can also be configured declaratively:

```yaml
model: company::coder
providers:
  company:
    name: Company Gateway
    protocol: openai_compatible
    base_url: https://llm.example.com/v1/
    credential:
      env: COMPANY_LLM_API_KEY
    models: [coder]
```

Exactly one credential source is required: `env` or `file`. `env`
contains the environment variable's **name**, not its value. For example,
`env: OPENAI_API_KEY` resolves the
current value when a request starts, so key rotation does not require a config edit.

Never place a raw key in `config.yaml`. File references point to an entry in
`credentials.yaml` in the same configuration directory. That fallback is unencrypted plaintext, created
atomically with mode 0600 on Unix; Queditor rejects a permissive or symlinked file.
Use `/connect delete <provider-id>` to remove a provider and its file secret.
Environment variables are never modified by Queditor.

## Editing bindings

Place overrides in `keybindings/<profile>.yaml` beneath the same configuration
directory. For example, `keybindings/vscode.yaml`:

```yaml
editor:
  ctrl+u: undo
  ctrl+r: redo
  alt+c: copy
  ctrl+x: none
```

Built-in profiles are loaded first, then overrides. A custom profile starts from
the vscode-style bindings. Keys use lower-case names and modifiers in
`ctrl+alt+shift+key` order; printable characters and arrow/home/end/delete/backspace
names are supported by event normalization. Global Ctrl+S, Ctrl+Q and Tab are
reserved and cannot be overridden by editor bindings. Multi-key chords and
chat-specific custom binding maps are not implemented.

Available editor actions: `left`, `right`, `up`, `down`, `home`, `end`, `delete`,
`backspace`, `kill_line`, `mark`, `select_all`, `cut`, `copy`, `paste`, `undo`,
`redo`, `none`.
Unknown actions reject the profile. `/keys` shows resolved bindings.

### Default profile subsets

VS Code: Ctrl+A/E line start/end, Ctrl+K deletes from the cursor to line end,
Ctrl+C/X/V internal copy/cut/paste, Ctrl+Z undo, Ctrl+Y or Ctrl+Shift+Z redo.

Emacs: Ctrl+B/F character movement, Ctrl+P/N line movement, Ctrl+A/E line
start/end, Ctrl+K delete to line end, Ctrl+D delete, Ctrl+H backspace, Ctrl+Space mark, Ctrl+W cut,
Alt+W copy, Ctrl+Y paste, Ctrl+Z undo. Escape clears the mark. These are familiar
editing subsets, not complete emulator/key-chord compatibility.

## Custom agents

Built-in agents live in the repository's `assets/agents/`, one Markdown file per
agent: `01-planner.md`, `02-blogger.md`, and any additional definitions. YAML frontmatter holds `name` and an
optional `description`; the Markdown body is the prompt, using the same format as
custom agents below. Names must be nonempty and unique (case-insensitive), and
built-in prompts must be nonempty.

The build automatically embeds all `.md` files in filename order, which is also
picker/rotation order. Numbered filenames keep the order explicit. Edit a file or
add another (for example, `03-reviewer.md`) and rebuild Queditor—no Rust registry
changes are needed. Installed binaries need no external copy of the files. This
does not modify your home-directory configuration.

For runtime custom agents, create `agents/copyeditor.md` beneath the configuration directory:

```markdown
---
name: Copyeditor
description: Reviews clarity and consistency
---

Preserve the writer's meaning and voice. Identify unclear passages and claims
that need verification. Suggest changes rather than silently rewriting.
```

Names must be nonempty and unique, including against all built-in definitions. Files with
invalid frontmatter produce a warning while valid files continue loading.
`/agents` opens the picker; `/agents Copyeditor` selects one. Shift+Tab cycles all
agent/mode pairs: Chat then Agent for each loaded agent before moving to the next.
Custom agents join automatically after the built-ins in filename order. Ctrl+G
cycles modes for the current agent only. Selection and per-agent modes are saved.

In live mode, the selected agent's instructions become the system prompt. Agent
instructions never override local permissions or edit-review validation.
API keys come from provider environment variables, never this YAML.
See [live AI](live-ai.md) for provider setup, privacy, and permissions.
