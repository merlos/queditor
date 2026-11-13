# Contributor and agent instructions

## Product and current scope

Queditor is a Rust terminal Markdown/text editor. Read `MASTER.md` for actual
feature status, `Masterplan.md` for the delivery roadmap, and `README.md` for
usage. The default assistant is mock; live mode uses the `genai` multi-provider
library. Never imply mock responses are live AI or every provider is account-tested.

## Architecture

- `src/main.rs`: CLI, terminal lifecycle, event loop; restore terminal state on errors.
- `src/app.rs`: application state, actions, commands, context, embedded/default and custom agents, mock proposals.
- `assets/agents/*.md`: built-in YAML-frontmatter Markdown agent definitions; `build.rs` embeds all files in filename/picker/rotation order. Numbered filenames preserve the built-in order.
- `src/document.rs`: Unicode-aware document operations, history, safe file handling.
- `src/config.rs`: embedded defaults, YAML profiles, semantic theme colors.
- `src/backend.rs`: genai streaming/tool loop, provider usage/timing, budgets, timeouts, sanitized errors.
- `src/provider.rs`: validated provider endpoints, model discovery, exact genai targets.
- `src/credentials.rs`: environment/private-file secret resolution and storage.
- `src/live.rs`: async job lifecycle, snapshot validation, permission/state orchestration.
- `src/review.rs`: already-applied edit ranges, manual-edit rebasing, protected Keep/Undo state.
- `src/tools.rs`: exact-path/URL grants, sensitive exclusions, anchored file and prepared web capabilities.
- `src/web.rs`: permission-gated public HTTP(S) GETs, pinned/validated DNS, bounded UTF-8 responses and sanitized errors.
- `src/storage.rs`: private versioned recovery/session/permission records and recovery-picker metadata.
- `src/ui.rs`: rendering and layout; do not perform filesystem/network I/O here.
- `src/lib.rs`: reusable modules for tests and future clients.
- `website/`: Astro/Starlight marketing and documentation site; `docs/` remains
  the canonical Markdown source and is copied at build time.

Keep document semantics independent of terminal rendering. Extract modules as
they acquire real complexity; do not create empty placeholder hierarchies.

## Conventions and safety

- Rust 2024, stable toolchain, `cargo fmt`, typed state, explicit errors.
- Register new agent modes in `AgentMode::ALL`; Shift+Tab and Ctrl+G share this order. Persist agent/mode pair changes together before updating application state.
- Cursor/selection offsets are UTF-8 bytes at grapheme boundaries.
- Display columns differ from byte/grapheme offsets; account for tabs and wide glyphs.
- Ordinary assistant text never mutates documents. A structured live Agent edit applies
  and saves automatically only after successful completion and snapshot validation.
- Validate base revision and original content before automatic application. Unnamed
  documents auto-save to private temporary backing files without acquiring a user
  filename; explicit Save As migrates to the real file for subsequent auto-saves.
- Automatic apply/save is transactional, one undoable edit, with disk-baseline checks.
  Failure must leave the editor and destination unchanged.
- Live pending edits are already on disk. Keep clears review state; review Undo saves
  a targeted reversal, checks original replacement content, and preserves manual edits.
- Rebase untouched review ranges after manual edits; protect overlapping edits from
  review Undo. Ordinary editor undo/redo remains unsaved until an explicit save.
- Offline /demo and /rewrite remain unsaved mock previews; saving excludes them.
- Do not overwrite external changes or discard dirty content without confirmation.
- Network/provider integration must go behind a backend boundary and never block rendering.
- Agent tools must enforce permissions locally, not through prompt instructions.
- Web tools must obtain exact-URL permission before DNS/network access; reject private/non-public destinations and never forward provider credentials or follow unapproved redirects.
- File capabilities must not read content before approval; keep them anchored across permission waits.
- Chat mode is read-only. Agent mode may target the complete open document snapshot;
  current selections and pinned snapshots are important context, not edit boundaries.
- Cancellation must discard generated proposals and prevent late events mutating a new session/document.
- Keep/Undo uses dedicated modified shortcuts. Typing edits the live document;
  deleted diff rows and headers are never editable document positions.
- Compaction must reject tool calls, even if a provider returns them without declared tools.
- Session-content persistence is opt-in; never persist temporary grants or credentials.
- Recovery writes local state, not the original document; restoration is explicit.
  With recovery enabled, durable independent pre-edit checkpoints must succeed before
  auto-save and survive Keep/normal exit. Never persist credentials or temporary grants.
- Never log document contents, prompts, credentials, or authorization headers by default.
- Configuration examples must not contain secrets.
- Do not edit users' home-directory configuration as part of repository development.

## Required checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --check
python3 tests/terminal_smoke.py
python3 tests/release_pipeline.py
npm --prefix website ci
npm --prefix website run build
```

Add regression tests for bug fixes. Prioritize Unicode, data-loss prevention,
stale context/proposals, command parsing, terminal layouts, and terminal cleanup.
Use temporary directories for filesystem tests. Do not rely on network credentials.
The Unix PTY smoke tests require the debug binary (`cargo build` or `cargo run`).

## Documentation upkeep

Each feature change must update related tests, the implemented/planned status in
`MASTER.md`, and the corresponding user documentation in `docs/`. Update
`Masterplan.md` when architecture or milestone scope changes. Keep README links
and limitations accurate. Update this file when module boundaries or conventions change.
