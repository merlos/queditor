# Queditor architecture and delivery roadmap

## Product decisions

- Rust terminal application, one UTF-8 document at a time.
- Application/configuration name: `queditor`; older `agenditor` names are draft typos.
- Default theme: Catppuccin-inspired Mocha; contrasting daylight theme.
- Preserve Markdown syntax in the editor; this is not a rich-text preview.
- VS Code-style live Agent workflow: validated structured edits apply and save before
  editable inline Keep/Undo review. Chat is read-only; mock previews remain unsaved.
- Keep mock mode as default; use `genai` for many providers behind one async boundary.
- OpenCode is an interaction reference, not a bundled dependency or a promise of parity.

## Current architecture

`main` owns CLI and terminal lifecycle. Crossterm events enter `App`, which applies
document or command actions. Ratatui renders application state. `Document` owns
text, grapheme-safe byte offsets, snapshots, saved/disk baselines, and revisions.
`Config` loads defaulted YAML settings and editor-action bindings. The UI handles
screen-column calculations independently of document byte offsets.
Editor/review, composer, and message rendering share source-preserving word-wrap
row calculations in `app`; editor tabs retain logical-line stops while prompt
and message tabs use visual-row stops. Words wider than the pane fall back to
grapheme-safe splitting. Selection and mouse mapping use the same row boundaries.

The implementation uses substantive modules instead of empty placeholder folders.
It uses `String` plus bounded snapshots rather than a rope; measure large-file
latency before switching storage. `backend` runs genai streaming and tool rounds
on Tokio, communicating through bounded channels with `live` orchestration.
`provider` validates endpoint profiles and creates exact genai targets; `credentials`
resolves environment and private-file references without exposing keys.
`tools` prepares anchored filesystem capabilities before permission waits, without
reading text and prepares exact-URL web capabilities without DNS/network access.
`web` performs approved, bounded public HTTP(S) GETs with validated/pinned DNS,
no credentials/proxies/redirects, and sanitized failures. `storage` handles private
recovery/session/grant records outside the renderer. `review` owns current edit
ranges and manual-edit rebasing/protection;
`Document::replace_and_save` commits editor changes only after a successful save.
Never call providers inside rendering or synchronous key handling.

### Invariants

1. Editing offsets are UTF-8 grapheme boundaries.
2. Dirty state compares content to the saved baseline, not just a boolean toggle.
3. Attachments capture exact text and document revision.
4. Agent requests hold complete document snapshots; completed structured edits must
   match revision/original before application.
5. Live application is one undoable, transactional auto-save. Keep clears review state;
   review Undo saves a targeted reversal and preserves later manual edits.
6. Saves check external disk changes. Live saves include all current text; offline
   mock previews remain excluded. Unnamed documents use private temporary backing until
   explicit Save As switches subsequent auto-saves to the user's file.
7. Dirty open/quit operations require explicit confirmation.
8. Mock mode makes no network calls; Chat mode sends explicit context, while Agent
   mode deliberately sends the open document snapshot plus approved tool results.
9. File permissions are local/exact-path and cannot be overridden by agent prompts.
   Browsing likewise requires local exact-URL approval before DNS or HTTP requests.
10. Partial/cancelled/failed turns never apply generated edits or commit model-facing history.
11. Recovery never writes the original document; session-content persistence is opt-in.
    Enabled recovery requires durable pre-edit checkpoints before auto-save, retained
    across Keep/exit. Pending live edits are already visible to external applications.

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

Shift+Tab rotates the loaded agent list crossed with the central `AgentMode::ALL`
order, visiting every mode before advancing agents. Custom agents join automatically;
new modes register in that shared order, also used by Ctrl+G. Agent/mode preferences
commit in one config save before changing application state; pane focus stays unchanged.
Built-in agents each have a YAML-frontmatter Markdown file in `assets/agents/`.
`build.rs` discovers and embeds every `.md` file in filename order, watching for
edits/additions/removals. Numbered filenames preserve Planner/Blogger ordering.
The same parser handles runtime custom Markdown files appended after the built-ins.

### 4. Mock chat/context — basic implementation complete

Composer, slash suggestions, scrollback, agent registry, session reset,
selection/document snapshots, and local mock edit demonstrations.
Prompt selection uses grapheme-safe byte offsets with keyboard/mouse editing and
the editor's shared internal clipboard. Rendered chat rows map back to sanitized
message source for read-only mouse selection/copy, preserving logical newlines
and Markdown while excluding headings, activity and model footers.
The editor/prompt/submitted messages use lighter borderless theme surfaces; the
prompt identifies agent/model, and response footers use per-message model metadata
captured at response creation and preserved by optional session persistence.
Live response footers also preserve provider-reported input/output usage across
tool rounds and output throughput over provider request time. Application-run usage
totals and live Unicode document statistics appear in a wrapping bottom status bar.
Recovery selection uses a newest-first keyboard/mouse popup, with storage preparing
snapshot metadata before rendering and restoration retaining dirty-document guards.
Chat reply rendering now uses `pulldown-cmark` source offsets for Markdown colors,
including streamed fragments and nested styles across Unicode-aware soft wraps.
The editor's existing lightweight source highlighting remains separate; neither
renderer performs link/image fetching or language-specific code highlighting.

Live streaming/tool integration, context budgets and optional persistent sessions
have now been added. Argument completion and broader chat-interaction parity remain.

### 5. Safe edit review — local workflow implemented

Offline deterministic proposals retain explicit acceptance and save exclusion.
Live edits now use an editable unified inline diff with red removals, green additions,
old/new line numbers, word highlights, context expansion/collapse, and Keep/Undo
navigation. Application and reversal save transactionally; manual changes outside
pending hunks rebase them, while overlapping edits are protected from review Undo.
Generation still rejects stale snapshots. Review does not delay saving or require Git.

### 6. Live AI and permissions — core implementation delivered

1. Define `Backend` request, streamed event, cancellation, model and error contracts.
2. Move effects off the terminal event loop using Tokio and bounded queues.
3. Add deterministic streaming/error/tool mocks and reconnection tests.
4. Use genai for multiple native/provider-compatible protocols and environment credentials.
5. Give each agent a persisted read-only Chat mode and full-document Agent mode.
6. Define and validate complete-document `propose_edit` output against captured revisions.
7. Introduce read/list tools with canonical paths, symlink checks, size/text limits,
   sensitive-file exclusions and scoped permission prompts.
8. Add allow-once/session/persistent/deny plus saved-grant review and revocation.
9. Do not grant shell execution, deletion, or arbitrary write tools. Queditor alone
   applies/saves a validated complete-document edit to the open file or private
   temporary backing for an unnamed document.
10. Add guided provider connection, endpoint-bound credentials, model discovery and
    selection, compaction, cancellation and sanitized provider failures.
11. Add simple public-web fetching with exact-URL browsing permission before network
    access, bounded text results, pinned public DNS answers, and no redirects/credentials.

The request/event contracts, Tokio jobs, bounded queues, structured proposal loop,
permissions/grant revocation, tool capability limits, provider setup/model discovery,
model selection and compaction
are implemented. Local HTTP/SSE provider fixtures exercise the real genai adapter
without credentials. Remaining: more provider-account testing, usage reporting,
automatic reconnect semantics and granular grant revocation.

Gate: mock/provider tests prove agents cannot read unapproved files, silently send
unattached document text, overwrite newer user/disk edits, or auto-save incomplete
results. Review Undo must preserve later manual edits and pre-edit checkpoints must
survive clean exit when recovery is enabled.

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
The Astro/Starlight marketing and documentation site deploys independently to
GitHub Pages, with repository `docs/` Markdown kept as its canonical content.

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
