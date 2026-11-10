# Queditor functional specification

## Intent

Provide a keyboard- and mouse-friendly terminal editor for Markdown/plain text,
with an integrated assistant that proposes changes for explicit user review.
The baseline draft is `../queditor.md`; naming is normalized to `queditor`.

Status: **Implemented** means present in the local prototype, **Partial** means
the stated subset exists, and **Planned** means not yet delivered.

| ID | Requirement | Status and acceptance contract |
|---|---|---|
| ED-001 | UTF-8 editing | Implemented: insert/delete/navigation/selection operate at grapheme boundaries. |
| ED-002 | Undo/redo | Implemented: accepted proposals and replace-all are single undoable transactions; bounded snapshot history. |
| ED-003 | Familiar shortcuts | Partial: VS Code/Emacs-style subsets and YAML editor-action overrides; no Emacs chords. |
| ED-004 | Mouse | Implemented: editor click/drag/wheel; explorer click selection and wheel; Enter opens selected entry. |
| ED-005 | Markdown/frontmatter | Partial: visible-source basic headings, emphasis, code, frontmatter colors; not a full parser. |
| ED-006 | Clipboard | Partial: internal copy/cut/paste and terminal bracketed paste; OS clipboard pending. |
| ED-007 | Search/replace | Implemented: `/find` and undoable `/replace` all, via chat commands. |
| UI-001 | Responsive panes | Implemented: at least 80/40 content columns at widths >=124; stacked below that; Unicode-aware editor soft wrapping; minimum 30×10; consistently colored one-answer-per-line popups with shortcuts, arrows/Enter, and mouse selection. |
| UI-002 | Status | Implemented: dirty filename, line/column, focus, actual model/agent, running state, labeled severity colors. |
| UI-003 | Themes | Partial: embedded Mocha/daylight; runtime selection and startup config; custom palettes pending. |
| FILE-001 | Open/save | Implemented: open UTF-8, create missing paths, save explicitly; directory must exist. |
| FILE-002 | Data-loss guards | Implemented: dirty confirmation, disk-baseline checks before save, no overwrite of another existing path. |
| FILE-003 | Explorer | Implemented: flat current folder, parent entry, folders first, arrows/Enter/Escape, preserves chat. |
| FILE-004 | Recovery | Implemented: private accepted-text checkpoints, explicit restore/delete, no implicit document writes. |
| CHAT-001 | Composer | Implemented: Enter submit, Ctrl+J newline, Shift+Enter when supported, soft-wrapped input, boundary-aware Up/Down prompt recall, slash completion, scrollable history. |
| CHAT-002 | Commands | Implemented: local commands, keyboard/mouse guided provider and credential selection with visible text cursor and masked typing/paste, persisted agent/model/per-agent mode selection, configured/arbitrary and discovered models, live compaction and cancellation; discovery depends on provider model-list support. |
| CHAT-003 | Sessions | Implemented: optional private history persistence, list/resume-as-new/delete; no temporary grant or pending proposal restoration. |
| CHAT-004 | Live streaming/cancel | Implemented: genai multi-provider streaming on Tokio, bounded queues/budgets, animated activity states, universal Escape cancellation, extended request/stream timeouts, safe error messages. |
| CTX-001 | Context | Implemented: selected-line indicator, automatic immutable selection snapshot in Chat mode, complete-document snapshot in Agent mode, explicit pinned context, stale warning, clear action. |
| AGENT-001 | Built-in agents | Implemented: selected Planner/Blogger/custom instructions are sent as the live system prompt. |
| AGENT-002 | Custom agents | Implemented: YAML-frontmatter Markdown definitions; invalid files warn without dropping valid definitions. |
| SAFE-001 | Proposal separation | Implemented: per-agent read-only Chat and full-document Agent modes; structured proposals require successful completion and explicit diff review. |
| SAFE-002 | Review | Implemented: non-modal proposed-document preview with inline replacement highlighting and bottom acceptance controls, enhanced-keyboard and command-based individual keep/reject plus keep/reject all, stale revision/original validation, atomic acceptance is undoable and unsaved. |
| SAFE-003 | Save accepted text only | Implemented: a save never includes an unaccepted proposal. |
| SAFE-004 | Agent file permissions | Implemented: read/list ask-by-default; once/session/saved exact-path grants, revocation, sensitive exclusions, anchored capabilities and size limits; no shell/writes. |
| CFG-001 | YAML defaults | Partial: embedded defaults, validated provider profiles, environment/keyring/private-file credential references, safe atomic updates and fallback warnings; CLI overrides pending. |
| DOC-001 | Documentation | Implemented: README, MASTER, Masterplan, AGENTS, user/config guides. |

Terminal startup/edit/save/review/quit workflows and restoration are covered by
Unix PTY smoke tests in addition to the document/application/rendering tests.

## Command semantics

The command registry in `src/app.rs` drives help and completion. Paths are resolved
relative to the chat workspace; quote spaces. `/cd` changes the workspace, not the
already-open document path. `/save` with no path saves the current document; an
unnamed document requires a path. Opening or quitting with dirty text/pending
review requires save/discard/cancel. Saving-and-continuing saves accepted content
and discards any proposal as the operation proceeds.

`/demo` uppercases the attached snapshot, current selection, or full document.
`/rewrite` uses supplied replacement text. Neither invokes an LLM. A stale
attachment is rejected; a stale proposal cannot be applied.

Live mode is configured with `/connect` and selected by `/models`, direct
`/models <provider::model>`, or startup YAML `model`. Credentials resolve from the
environment, OS keyring, or an explicitly selected private plaintext file. Only current
selections, pinned attachments, and approved tool results supplement the live
prompt/history.
Escape from either pane, `/cancel`, and Ctrl+C in chat discard unfinished generated proposals; model-facing
history commits only after success. `/compact` summarizes history without tools.
`/permissions` shows grants; `clear` revokes them. `/new`, `/cd`, `/resume` clear
temporary grants. `/recover` and `/sessions` manage private local records; resume
does not open the document and restore never writes it. See `docs/live-ai.md`.

## Release boundary

The main AI workflow now exists through genai, with mock mode retained. This is
still an early prototype: provider-account compatibility, stronger terminal/signal
testing, OS clipboard, full Markdown parsing, wrapping/indent controls, custom
themes/key chords, and large-file latency remain unfinished. Native unsigned
release archives and checksums are automated; signing, notarization, and package-
manager formats remain future work. The marketing site and searchable Markdown
documentation are automatically deployed to GitHub Pages.
