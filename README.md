# Queditor

A Rust terminal Markdown/text editor with a side-by-side or stacked chat pane
and explicit review of assistant edits.

**Early local prototype:** the assistant is a mock backend. No prompts, files, or
credentials are sent over the network. Live AI and permission-controlled agent
tools are not implemented yet.

## Run

Install a current stable Rust toolchain, then:

```sh
cargo run -- path/to/document.md
```

Omit the path to start an unnamed document. A missing filename creates a new
document if its parent directory exists. To install the binary:

```sh
cargo install --path .
queditor document.md
```

An interactive terminal is required. `cargo run -- --check` checks startup without
entering the UI. `queditor --help` shows CLI options.

## Quick start

- Type in the editor; use arrows and Shift+arrows to move/select.
- **Tab** switches panes. **Ctrl+S** saves; **Ctrl+Q** exits safely.
- In chat, **Enter** submits and **Ctrl+J** inserts a newline. Shift+Enter also
  works if your terminal reports it distinctly.
- `/files` browses files; `/open "a file.md"` opens/creates a document.
- `/save "draft.md"` saves an unnamed document.
- Select text in the editor, switch to chat, and use `/attach` to pin a snapshot.
- `/demo` creates a deterministic uppercase proposal, not an AI response.
- **A/Enter** accepts a proposal; **R/Esc** rejects it. **Ctrl+Z** undoes acceptance.
- `/rewrite "replacement text"` previews a replacement for selection/context,
  or the whole document when neither is present.
- `/help` lists commands; `/keys` shows the current editing bindings.

## Current features

- UTF-8 text editing, grapheme-aware selection, mouse selection and wheel scrolling.
- Undo/redo, internal copy/cut/paste, terminal bracketed paste.
- Basic source-preserving Markdown/frontmatter colors.
- Wide/narrow layouts, status messages, file explorer, search/replace commands.
- Mocha/daylight themes, VS Code-style and Emacs-style editing presets.
- YAML configuration and custom Markdown agent definitions.
- Attached context snapshots and stale-proposal checks.
- Temporary-file saves, newline preservation, and disk-change checks at save time.

## Important limitations

- No live AI, tool permissions, model selection, real compaction, or session persistence.
- Copy/cut shortcuts use **Queditor's internal clipboard**, not the OS clipboard.
  Your terminal's paste action is supported and never submits pasted newlines.
- One document and one modal proposal at a time; reject/accept before editing again.
- No soft wrapping in the editor, per-hunk acceptance, or automatic recovery.
- UTF-8 only; files over 8 MiB and files containing NUL bytes are rejected.
- Basic highlighting is not a full Markdown parser.
- Emacs bindings are a familiar subset, not full Emacs/chord emulation.
- Atomic replacement does not guarantee preservation of hard links, ACLs, xattrs,
  or concurrent writes occurring after the final disk check.

## Documentation

- [User guide](docs/user-guide.md)
- [Configuration and custom agents](docs/configuration.md)
- [Functional specification and feature status](MASTER.md)
- [Architecture and delivery roadmap](Masterplan.md)
- [Contributor/agent conventions](AGENTS.md)

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
python3 tests/terminal_smoke.py
```
