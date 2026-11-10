# Queditor

A Rust terminal Markdown/text editor with a side-by-side or stacked chat pane
and explicit review of assistant edits.

**Early prototype:** offline mock mode is the default. Live AI uses
[`genai`](https://github.com/jeremychone/rust-genai), a multi-provider Rust library
supporting OpenAI, Anthropic, Gemini, Ollama, OpenRouter, Groq, and more.
Live requests send chat history, explicitly attached text, and approved file-tool
results to the selected provider. Edits still require explicit review.

## Run

### Download a binary

Unsigned binaries are published on the repository's
[GitHub Releases](https://github.com/merlos/queditor/releases) page for:

- Windows x86-64 (`.zip`)
- Linux x86-64, statically linked (`.tar.gz`)
- macOS Intel x86-64 (`.tar.gz`)
- macOS Apple Silicon arm64 (`.tar.gz`)

Download the archive for your platform, verify it against `SHA256SUMS` (or its
adjacent `.sha256` file), extract it, and run `queditor` from a terminal. On
Windows, run `queditor.exe` from PowerShell or Command Prompt. The first unsigned
release may trigger Windows SmartScreen or macOS Gatekeeper; inspect the checksum
and release source before choosing to run it. Code signing and notarization are
planned, not currently provided.

On macOS, an unsigned downloaded binary may need explicit approval in **System
Settings → Privacy & Security**. Do not bypass security warnings for a binary whose
checksum and release source you have not verified.

### Build from source

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
- Live streaming, cancellation, model switching, and conversation compaction.
- Read/list tools with once/session/exact-path persistent permission choices.
- Private recovery checkpoints and opt-in saved sessions.

## Enable live AI

Provide your provider's API key through its documented environment variable,
then select a model in chat, for example `/models openai::gpt-4o-mini`.
For local Ollama, use `/models ollama::llama3.2` (the model must be installed).
Use a model available to your account; tool/stream support varies by model.

Select editor text and `/attach`, then ask for a revision. The assistant can
propose a replacement for that exact snapshot. Editing while it runs is allowed;
stale results will not be applied. For live proposals, Ctrl+R enables acceptance,
then A/Enter accepts. `/cancel` or Ctrl+C in chat cancels generation.

See [provider setup and privacy](docs/live-ai.md) for authentication, custom
endpoints, permissions, sessions, and recovery details.

## Important limitations

- Copy/cut shortcuts use **Queditor's internal clipboard**, not the OS clipboard.
  Your terminal's paste action is supported and never submits pasted newlines.
- One document and one modal proposal at a time; reject/accept before editing again.
- No soft wrapping in the editor or per-hunk acceptance. Recovery checkpoints
  run approximately once a second after changes; the latest uncheckpointed input
  can still be lost. Saved session history is disabled by default.
- UTF-8 only; files over 8 MiB and files containing NUL bytes are rejected.
- Basic highlighting is not a full Markdown parser.
- Emacs bindings are a familiar subset, not full Emacs/chord emulation.
- Atomic replacement does not guarantee preservation of hard links, ACLs, xattrs,
  or concurrent writes occurring after the final disk check.
- Tool permissions are application-level controls, not an OS-wide sandbox.
- Model lists are configuration suggestions, not provider-account discovery.
- Provider-account compatibility and automatic reconnection/retry still need hardening.

## Documentation

- [User guide](docs/user-guide.md)
- [Configuration and custom agents](docs/configuration.md)
- [Live AI, permissions, sessions, and recovery](docs/live-ai.md)
- [Release process](docs/releasing.md)
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
