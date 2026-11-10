# Contributor and agent instructions

## Product and current scope

Queditor is a Rust terminal Markdown/text editor. Read `MASTER.md` for actual
feature status, `Masterplan.md` for the delivery roadmap, and `README.md` for
usage. The default assistant is mock; live mode uses the `genai` multi-provider
library. Never imply mock responses are live AI or every provider is account-tested.

## Architecture

- `src/main.rs`: CLI, terminal lifecycle, event loop; restore terminal state on errors.
- `src/app.rs`: application state, actions, commands, context, agents, mock proposals.
- `src/document.rs`: Unicode-aware document operations, history, safe file handling.
- `src/config.rs`: embedded defaults, YAML profiles, semantic theme colors.
- `src/backend.rs`: genai streaming/tool loop, budgets, timeouts, sanitized errors.
- `src/live.rs`: async job lifecycle, snapshot validation, permission/state orchestration.
- `src/tools.rs`: exact-path grants, sensitive exclusions, anchored file capabilities.
- `src/storage.rs`: private versioned recovery/session/permission records.
- `src/ui.rs`: rendering and layout; do not perform filesystem/network I/O here.
- `src/lib.rs`: reusable modules for tests and future clients.

Keep document semantics independent of terminal rendering. Extract modules as
they acquire real complexity; do not create empty placeholder hierarchies.

## Conventions and safety

- Rust 2024, stable toolchain, `cargo fmt`, typed state, explicit errors.
- Cursor/selection offsets are UTF-8 bytes at grapheme boundaries.
- Display columns differ from byte/grapheme offsets; account for tabs and wide glyphs.
- Never mutate a document merely because an assistant returned text.
- Validate base revision and original content before accepting a proposal.
- Acceptance is one undoable edit and never an implicit save.
- Saving excludes pending proposals and checks the disk baseline.
- Do not overwrite external changes or discard dirty content without confirmation.
- Network/provider integration must go behind a backend boundary and never block rendering.
- Agent tools must enforce permissions locally, not through prompt instructions.
- File capabilities must not read content before approval; keep them anchored across permission waits.
- Only explicitly attached editor snapshots may be targets for live proposals.
- Cancellation must discard generated proposals and prevent late events mutating a new session/document.
- Live proposal acceptance requires an explicit review action before ordinary A/Enter keys.
- Compaction must reject tool calls, even if a provider returns them without declared tools.
- Session-content persistence is opt-in; never persist temporary grants or credentials.
- Recovery writes local state, not the original document; restoration is explicit.
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
