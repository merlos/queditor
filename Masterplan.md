# Queditor architecture and delivery roadmap

## Product decisions

- Rust terminal application, one UTF-8 document at a time.
- Application/configuration name: `queditor`; older `agenditor` names are draft typos.
- Default theme: Catppuccin-inspired Mocha; contrasting daylight theme.
- Preserve Markdown syntax in the editor; this is not a rich-text preview.
- Agent output is a proposal, never permission to mutate or save a document.
- Keep mock mode as default; use `genai` for many providers behind one async boundary.
- OpenCode is an interaction reference, not a bundled dependency or a promise of parity.

## Current architecture

`main` owns CLI and terminal lifecycle. Crossterm events enter `App`, which applies
document or command actions. Ratatui renders application state. `Document` owns
text, grapheme-safe byte offsets, snapshots, saved/disk baselines, and revisions.
`Config` loads defaulted YAML settings and editor-action bindings. The UI handles
screen-column calculations independently of document byte offsets.

The implementation uses substantive modules instead of empty placeholder folders.
It uses `String` plus bounded snapshots rather than a rope; measure large-file
latency before switching storage. `backend` runs genai streaming and tool rounds
on Tokio, communicating through bounded channels with `live` orchestration.
`tools` prepares anchored filesystem capabilities before permission waits, without
reading text. `storage` handles private recovery/session/grant records outside the
renderer. Never call providers inside rendering or synchronous key handling.

### Invariants

1. Editing offsets are UTF-8 grapheme boundaries.
2. Dirty state compares content to the saved baseline, not just a boolean toggle.
3. Attachments capture exact text and document revision.
4. Proposals hold original text, range, revision, and replacement separately.
5. Acceptance validates revision/original and creates one undoable edit.
6. Save operates only on accepted text and checks external disk changes.
7. Dirty open/quit operations require explicit confirmation.
8. Mock mode makes no network calls; live mode sends explicit context and approved tool results only.
9. File permissions are local/exact-path and cannot be overridden by agent prompts.
10. Partial/cancelled/failed turns never stage generated edits or commit model-facing history.
11. Recovery never writes the original document; session-content persistence is opt-in.

## Milestones

### 0. Specification and risk prototypes — initial decisions complete

The draft is normalized in `MASTER.md`. Unicode, narrow/wide rendering, paste,
proposal safety, and save-baseline behavior have automated coverage. Broader
terminal modifier-key compatibility still needs testing on real terminals.

### 1. Foundation — implemented

Cargo project, terminal restoration, state/event dispatch, responsive panes,
focus, status, tests, embedded settings, and contributor documentation.

### 2. Reliable local editor — substantially implemented

Editing/navigation, grapheme selection, mouse, bounded undo/redo, bracketed paste,
open/save safeguards, newline preservation, and command-based search/replace.

Remaining: OS clipboard, parser-based highlighting, wrapping/indent controls,
large-file performance, broader filesystem metadata preservation and live change notices.

### 3. Navigation/customization — basic implementation complete

Explorer, workspace commands, two themes, two editing profiles, custom editor
bindings, and cheatsheet. Remaining: custom theme files, key-sequence support,
comprehensive key validation, hot reload, and richer configuration merging.

### 4. Mock chat/context — basic implementation complete

Composer, slash suggestions, scrollback, agent registry, session reset,
selection/document snapshots, and local mock edit demonstrations.

Live streaming/tool integration, context budgets and optional persistent sessions
have now been added. Argument completion and broader chat-interaction parity remain.

### 5. Safe edit review — local workflow implemented

Deterministic proposals, colored diff, whole-proposal accept/reject, undo,
stale revision rejection, and save exclusion. Pending proposals are modal in this
build. Live generation allows concurrent editing and rejects stale results.
Per-hunk acceptance remains future work.

### 6. Live AI and permissions — core implementation delivered

1. Define `Backend` request, streamed event, cancellation, model and error contracts.
2. Move effects off the terminal event loop using Tokio and bounded queues.
3. Add deterministic streaming/error/tool mocks and reconnection tests.
4. Use genai for multiple native/provider-compatible protocols and environment credentials.
5. Send selected agent instructions and explicitly attached context only.
6. Define and validate structured `propose_edit` output against captured revisions.
7. Introduce read/list tools with canonical paths, symlink checks, size/text limits,
   sensitive-file exclusions and scoped permission prompts.
8. Add allow-once/session/persistent/deny plus saved-grant review and revocation.
9. Do not grant shell execution, deletion, or direct disk writes in the initial AI MVP.
10. Add model selection, compaction, cancellation and sanitized provider failures.

The request/event contracts, Tokio jobs, bounded queues, structured proposal loop,
permissions/grant revocation, tool capability limits, model selection and compaction
are implemented. Local HTTP/SSE provider fixtures exercise the real genai adapter
without credentials. Remaining: more provider-account testing, usage reporting,
automatic reconnect semantics, granular grant revocation, and model discovery.

Gate: mock/provider tests prove agents cannot read unapproved files, silently send
unattached document text, overwrite newer user edits, or write unaccepted proposals.

An optional OpenCode V2 HTTP adapter requires a separate authenticated discovery
and compatibility spike. Its experimental API surface must not be assumed stable,
and its tools must not bypass Queditor review or local permissions.

### 7. Release hardening — recovery/persistence implemented; remainder planned

Recovery, opt-in session persistence/deletion, local privacy modes, and unsigned
GitHub Release archives for Linux x86-64, Windows x86-64, Intel macOS, and Apple
Silicon macOS are implemented. Remaining: signed/notarized artifacts, package-manager
formats, clipboard support, large-file benchmarks, supported-terminal matrix,
broader signal/terminal compatibility, accessibility and complete release coverage.
Unix shutdown signals now checkpoint interrupted work and restore terminal state.

Gate: a new user can install, configure a provider, edit, attach context, approve
reads, request/review a revision, undo it, save, and recover interrupted work using
only the user documentation.

## Testing strategy

Run formatting, Clippy with warnings denied, unit/integration-style module tests,
and startup checks. Add property tests for edit/undo invariants, fixture/snapshot
tests for rendering and Markdown, mock HTTP tests for live backends, and PTY tests
for terminal lifecycle (initial Unix PTY workflows are implemented in
`tests/terminal_smoke.py`). Filesystem tests must use isolated temporary directories.

## Known tradeoffs

- Snapshot history is simple and safe but can be expensive for large documents;
  currently capped at 100 edits and 32 MiB of undo text.
- Save replaces a file after baseline checks; it is not an OS-wide file-locking
  protocol, and hard-link/ACL/xattr preservation is not guaranteed.
- Current highlighting is lexical/basic, not semantically complete Markdown.
- Provider compatibility varies; library support is not account-level validation.
- Local state is private plaintext, not encrypted. Recovery can miss the latest
  uncheckpointed second of input, and disk failures are warnings rather than guarantees.
- Filesystem permissions are application-level capabilities, not an OS-wide sandbox.

Keep this roadmap and `MASTER.md` synchronized as milestones become complete.
