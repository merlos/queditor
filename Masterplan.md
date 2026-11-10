# Queditor architecture and delivery roadmap

## Product decisions

- Rust terminal application, one UTF-8 document at a time.
- Application/configuration name: `queditor`; older `agenditor` names are draft typos.
- Default theme: Catppuccin-inspired Mocha; contrasting daylight theme.
- Preserve Markdown syntax in the editor; this is not a rich-text preview.
- Agent output is a proposal, never permission to mutate or save a document.
- Start with a mock backend. Implement one live provider behind an abstraction next.
- OpenCode is an interaction reference, not a bundled dependency or a promise of parity.

## Current architecture

`main` owns CLI and terminal lifecycle. Crossterm events enter `App`, which applies
document or command actions. Ratatui renders application state. `Document` owns
text, grapheme-safe byte offsets, snapshots, saved/disk baselines, and revisions.
`Config` loads defaulted YAML settings and editor-action bindings. The UI handles
screen-column calculations independently of document byte offsets.

The initial implementation deliberately has five substantive modules instead of
empty folders. It uses `String` plus bounded snapshots instead of a rope; measure
large-file latency before choosing incremental/rope storage. Network integration
must first introduce effects/events and an async backend boundary. Do not call
providers inside the renderer or synchronous key handling.

### Invariants

1. Editing offsets are UTF-8 grapheme boundaries.
2. Dirty state compares content to the saved baseline, not just a boolean toggle.
3. Attachments capture exact text and document revision.
4. Proposals hold original text, range, revision, and replacement separately.
5. Acceptance validates revision/original and creates one undoable edit.
6. Save operates only on accepted text and checks external disk changes.
7. Dirty open/quit operations require explicit confirmation.
8. No network calls, agent file tools, or credential reads exist in the prototype.

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

Composer, slash suggestions, scrollback, agent registry, in-memory session reset,
selection/document snapshots, honest unavailable-model/compaction responses.

Remaining: event-driven streaming/cancellation mock, persistent sessions, command
argument completion, and limits/policies for large attached context.

### 5. Safe edit review — local workflow implemented

Deterministic proposals, colored diff, whole-proposal accept/reject, undo,
stale revision rejection, and save exclusion. Pending proposals are modal in this
build; concurrent generation/editing and per-hunk review remain future work.

### 6. Live AI and permissions — next major milestone

1. Define `Backend` request, streamed event, cancellation, model and error contracts.
2. Move effects off the terminal event loop using Tokio and bounded queues.
3. Add deterministic streaming/error/tool mocks and reconnection tests.
4. Implement one OpenAI-compatible adapter with credential references, never secrets in YAML.
5. Send selected agent instructions and explicitly attached context only.
6. Define and validate structured `propose_edit` output against captured revisions.
7. Introduce read/list tools with canonical paths, symlink checks, size/text limits,
   sensitive-file exclusions and scoped permission prompts.
8. Add allow-once/session/persistent/deny plus saved-grant review and revocation.
9. Do not grant shell execution, deletion, or direct disk writes in the initial AI MVP.
10. Add real `/models`, compaction, cancellation and visible provider failures.

Gate: mock/provider tests prove agents cannot read unapproved files, silently send
unattached document text, overwrite newer user edits, or write unaccepted proposals.

An optional OpenCode V2 HTTP adapter requires a separate authenticated discovery
and compatibility spike. Its experimental API surface must not be assumed stable,
and its tools must not bypass Queditor review or local permissions.

### 7. Release hardening — planned

Recovery, opt-in session persistence/deletion, privacy controls, clipboard support,
large-file benchmarks, supported-terminal matrix, signal/terminal cleanup tests,
installation packages, release CI, accessibility, complete end-to-end coverage.

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
- Current chat is local and in-memory; interruption recovery is not available.

Keep this roadmap and `MASTER.md` synchronized as milestones become complete.
