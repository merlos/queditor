# Configuration

The app reads `$XDG_CONFIG_HOME/queditor/config.yaml`, or
`~/.config/queditor/config.yaml` when XDG_CONFIG_HOME is unset. It does not create
or modify home-directory configuration automatically.

```yaml
schema_version: 1
theme: mocha           # mocha or daylight
keybindings: vscode    # vscode, emacs, or a custom profile filename
model: mock            # or a genai provider::model identifier
models: [mock]         # suggestions shown by /models
persist_sessions: false
recovery: true
```

Missing fields inherit embedded defaults. Unknown fields, invalid themes, and
unsupported schema versions warn and fall back to safe defaults. A bad binding
profile falls back to vscode. Changes take effect on the next launch.
`/theme daylight` changes the theme for the current run only.

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
`backspace`, `mark`, `select_all`, `cut`, `copy`, `paste`, `undo`, `redo`, `none`.
Unknown actions reject the profile. `/keys` shows resolved bindings.

### Default profile subsets

VS Code: Ctrl+A select all, Ctrl+C/X/V internal copy/cut/paste, Ctrl+Z undo,
Ctrl+Y or Ctrl+Shift+Z redo.

Emacs: Ctrl+B/F character movement, Ctrl+P/N line movement, Ctrl+A/E line
start/end, Ctrl+D delete, Ctrl+H backspace, Ctrl+Space mark, Ctrl+W cut,
Alt+W copy, Ctrl+Y paste, Ctrl+Z undo. Escape clears the mark. These are familiar
editing subsets, not complete emulator/key-chord compatibility.

## Custom agents

Create `agents/reviewer.md` beneath the configuration directory:

```markdown
---
name: Reviewer
description: Reviews clarity and consistency
---

Preserve the writer's meaning and voice. Identify unclear passages and claims
that need verification. Suggest changes rather than silently rewriting.
```

Names must be nonempty and unique, including against Planner/Blogger. Files with
invalid frontmatter produce a warning while valid files continue loading.
`/agents` lists definitions; `/agents Reviewer` selects one. Shift+Tab cycles.

In live mode, the selected agent's instructions become the system prompt. Agent
instructions never override local permissions or edit-review validation.
API keys come from provider environment variables, never this YAML.
See [live AI](live-ai.md) for provider setup, privacy, and permissions.
