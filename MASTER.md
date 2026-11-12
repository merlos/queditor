# Queditor functional specification

## Intent

Provide a keyboard- and mouse-friendly terminal editor for Markdown/plain text,
with an integrated assistant that applies and saves live Agent edits, then offers
inline Keep/Undo review. Chat mode remains read-only; offline mock previews are unsaved.
The baseline draft is `../queditor.md`; naming is normalized to `queditor`.

Status: **Implemented** means present in the local prototype, **Partial** means
the stated subset exists, and **Planned** means not yet delivered.

| ID | Requirement | Status and acceptance contract |
|---|---|---|
| ED-001 | UTF-8 editing | Implemented: insert/delete/navigation/selection operate at grapheme boundaries. |
| ED-002 | Undo/redo | Implemented: live auto-apply/save and review Undo are transactional undoable edits; mock acceptance and replace-all are single undoable transactions; ordinary editor undo/redo does not auto-save; bounded snapshot history. |
| ED-003 | Familiar shortcuts | Partial: VS Code/Emacs-style subsets and YAML editor-action overrides; no Emacs chords. |
| ED-004 | Mouse | Implemented: editor click/drag/wheel; live review block selection and clickable per-block Keep/Undo labels; explorer click selection and wheel; Enter opens selected entry. |
| ED-005 | Markdown/frontmatter | Partial: visible-source basic headings, emphasis, code, frontmatter colors; not a full parser. |
| ED-006 | Clipboard | Partial: shared internal copy/cut/paste in editor and agent prompt, mouse selection/copy of read-only message source, and terminal bracketed paste; OS clipboard pending. |
| ED-007 | Search/replace | Implemented: `/find` and undoable `/replace` all, via chat commands. |
| UI-001 | Responsive panes | Implemented: at least 80/40 content columns at widths >=124; stacked below that; borderless lighter editor/prompt/user-message surfaces in both themes, standard-background agent replies; prompt footer separated by a blank row when space permits, prompt/editor panel bottoms aligned side-by-side; Unicode-aware editor soft wrapping; minimum 30×10; consistently colored one-answer-per-line popups with shortcuts, arrows/Enter, and mouse selection. |
| UI-002 | Status | Implemented: empty editor/assistant headers; bottom bar shows filename/dirty marker before line/column and labeled severity colors, without focus/agent/model/theme; distinct green You/accent agent labels, matching agent color in the prompt footer, with animated activity beneath the responding agent's name and a full-cycle spinner smoothly cycling through rainbow colors while activity text stays muted; prompt panel includes agent Chat/Agent · provider:model, completed responses end with their captured original model (legacy unattributed responses remain unlabeled, mock explicitly offline). |
| UI-003 | Themes | Partial: embedded Mocha/daylight; runtime selection and startup config; custom palettes pending. |
| FILE-001 | Open/save | Implemented: open UTF-8, create missing paths, save explicitly; unnamed saves via command, Ctrl+S, or save-and-continue open an editable filename popup with cancellation and retryable errors; directory must exist. |
| FILE-002 | Data-loss guards | Implemented: dirty confirmation, disk-baseline checks before save, no overwrite of another existing path. |
| FILE-003 | Explorer | Implemented: flat current folder, parent entry, folders first, arrows/Enter/Escape, preserves chat. |
| FILE-004 | Recovery | Implemented: private unsaved-text checkpoints and retained independent pre-agent-edit snapshots before auto-save when recovery is enabled; `/recover` opens a newest-first keyboard/mouse snapshot picker with identifier, age and document path; explicit restore/delete, restoration respects dirty confirmation and never writes the document. |
| CHAT-001 | Composer | Implemented: Enter submits the highlighted slash suggestion, Tab completes without submitting, Ctrl+J newline, Shift+Enter when supported, Ctrl+A/E move to logical line start/end and Ctrl+K deletes to line end without deleting the newline, soft-wrapped input, boundary-aware Up/Down prompt recall, scrollable history; grapheme-safe Shift+arrow/mouse prompt selection, shared clipboard copy/cut/paste and selection replacement, Ctrl+Shift+A select-all in prompt/clicked message, mouse selection/copy within read-only replies preserving Markdown source and logical newlines. |
| CHAT-002 | Commands | Implemented: local commands including `/version` for the build's package version, without user-message echoes, with errors above the prompt hints, keyboard/mouse guided provider and credential selection with visible text cursor and masked typing/paste, persisted agent/model/per-agent mode selection, configured/arbitrary and discovered models, live compaction and cancellation; discovery depends on provider model-list support. |
| CHAT-003 | Sessions | Implemented: optional private history persistence, list/resume-as-new/delete; no temporary grant or pending proposal restoration. |
| CHAT-004 | Live streaming/cancel | Implemented: genai multi-provider streaming on Tokio, bounded queues/budgets, animated activity states, universal Escape cancellation, 60-second connection/thirty-minute stream waits and a thirty-minute total deadline including permission waits, safe error messages. |
| CHAT-005 | Markdown replies | Implemented: parser-backed, source-preserving colors for bot headings, bold/italic, inline/fenced code, links/images, quotes, and strikethrough; shared word-boundary wrapping across editor/review, prompt, messages, statistics, activity, model footers, errors and wizard instructions, with grapheme-safe splitting only for overlong words and source-preserving selection/mouse offsets; reparsing streamed fragments; submitted/system text stays plain; no URL/image fetching or language-specific code highlighting. |
| CTX-001 | Context | Implemented: selected-line indicator, automatic immutable selection snapshot in Chat mode, complete-document snapshot in Agent mode, explicit pinned context, stale warning, clear action. |
| AGENT-001 | Built-in agents | Implemented: `/agents` opens a scrollable keyboard/mouse picker, `/agents <name>` selects directly; Shift+Tab cycles all loaded agent/mode pairs from either pane, visiting all registered modes before the next agent and wrapping; Ctrl+G cycles the current agent's modes using the same mode order; selection saves atomically; selected Planner/Blogger/custom instructions are sent as the live system prompt. |
| AGENT-002 | Agent definitions | Implemented: one YAML-frontmatter Markdown file per built-in in `assets/agents/`, discovered and embedded at compile time in filename order, validated for nonempty list/instructions and unique valid names; the same Markdown parser handles runtime custom definitions; invalid custom files warn without dropping valid definitions. |
| SAFE-001 | Agent edit lifecycle | Implemented: per-agent read-only Chat and full-document Agent modes; structured edits auto-apply/save only after successful completion and revision/original validation; unnamed documents use private temporary backing until explicit Save As, then subsequent auto-saves use the real file; ordinary response text cannot mutate the document. |
| SAFE-002 | Review | Implemented: editable unified inline diff with old/new line numbers, red removals, green additions, word highlights, full context or three-line collapsed context, cursor-driven block selection/highlighting, keyboard navigation, clickable per-block Keep/Undo labels and all-change commands; pending live edits already saved; manual overlap protection and untouched-range rebasing; auto-save failure leaves content unchanged. |
| SAFE-003 | Save semantics | Implemented: live Agent edits save the complete open document, including existing unsaved text, before review; Keep confirms, review Undo saves targeted reversal; external disk changes are never overwritten; mock previews remain excluded from saves. |
| SAFE-004 | Agent file permissions | Implemented: read/list ask-by-default; once/session/saved exact-path grants, revocation, sensitive exclusions, anchored capabilities and size limits; no shell/writes. |
| SAFE-005 | Web browsing tool | Implemented: `webfetch` in live Chat/Agent requests permission to browse each exact HTTP(S) URL before DNS/network access; once/session/saved grants; public addresses only, validated DNS pinned to the connection, no proxies/credentials/redirects; bounded 256 KiB UTF-8 text/HTML/JSON/XML and 30-second deadline; no JavaScript or automatic link/image fetching. |
| CFG-001 | YAML defaults | Partial: embedded defaults, validated provider profiles, environment/keyring/private-file credential references, safe atomic updates and fallback warnings; CLI overrides pending. |
| DOC-001 | Documentation | Implemented: README, MASTER, Masterplan, AGENTS, user/config guides. |
| UI-004 | Document statistics | Implemented: bottom bar shows Unicode word and grapheme character counts and live UTF-8 buffer size in decimal b/KB/MB/GB; wraps on narrow terminals; unsaved text included, mock/diff rows excluded. |
| CHAT-006 | Usage metrics | Implemented: live response model footers show provider-reported input/output tokens summed across tool rounds and output tk/s over provider request time, excluding tool/permission waits; bottom bar keeps observed application-run input/output totals across `/new` and resume. Missing usage is unknown, partial known totals are lower bounds; opt-in sessions preserve per-response metrics without replaying them into run totals. |

Terminal startup/edit/save/review/quit workflows and restoration are covered by
Unix PTY smoke tests in addition to the document/application/rendering tests.

## Command semantics

The command registry in `src/app.rs` drives help and completion. Paths are resolved
relative to the chat workspace; quote spaces. `/cd` changes the workspace, not the
already-open document path. `/save` with no path saves the current document; an
unnamed document prompts for a path in a popup. Opening or quitting with dirty text/pending
review requires save/discard/cancel. Saving-and-continuing saves current text;
discard affects only unsaved text and mock previews, not already-saved live edits.
Review state is cleared as the operation proceeds. Use `/changes undo` or `undo-all`
before leaving to reverse saved pending edits; `reject` aliases Undo.

`/demo` uppercases the attached snapshot, current selection, or full document.
`/rewrite` uses supplied replacement text. Neither invokes an LLM. A stale
attachment is rejected; a stale proposal cannot be applied.

Live mode is configured with `/connect` and selected by `/models`, direct
`/models <provider::model>`, or startup YAML `model`. Credentials resolve from the
environment, OS keyring, or an explicitly selected private plaintext file. Only current
selections, pinned attachments, and approved tool results supplement the live
prompt/history.
Escape from either pane, `/cancel`, and Ctrl+C in chat without selected text discard unfinished generated proposals; model-facing
history commits only after success. `/compact` summarizes history without tools.
`/permissions` shows grants; `clear` revokes them. `/new`, `/cd`, `/resume` clear
temporary grants. `/recover` and `/sessions` manage private local records; resume
does not open the document and restore never writes it. See `docs/live-ai.md`.

## Release boundary

The main AI workflow now exists through genai, with mock mode retained. This is
still an early prototype: provider-account compatibility, stronger terminal/signal
testing, OS clipboard, full Markdown parsing, wrapping/indent controls, custom
themes/key chords, and large-file latency remain unfinished. Native unsigned
release archives and checksums are automated; publishing reuses an existing release
for a tag and replaces matching artifacts, or creates it if missing. Signing,
notarization, and package-manager formats remain future work. The marketing site and
searchable Markdown documentation are automatically deployed to GitHub Pages.
