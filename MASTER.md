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
| UI-001 | Responsive panes | Implemented: at least 80/40 content columns at widths >=124; stacked below that; minimum 30×10. |
| UI-002 | Status | Implemented: dirty filename, line/column, focus, agent/mock model, labeled severity colors. |
| UI-003 | Themes | Partial: embedded Mocha/daylight; runtime selection and startup config; custom palettes pending. |
| FILE-001 | Open/save | Implemented: open UTF-8, create missing paths, save explicitly; directory must exist. |
| FILE-002 | Data-loss guards | Implemented: dirty confirmation, disk-baseline checks before save, no overwrite of another existing path. |
| FILE-003 | Explorer | Implemented: flat current folder, parent entry, folders first, arrows/Enter/Escape, preserves chat. |
| FILE-004 | Recovery | Planned: no autosave/recovery in current build. |
| CHAT-001 | Composer | Implemented: Enter submit, Ctrl+J newline, Shift+Enter when supported, slash completion, scrollable history. |
| CHAT-002 | Commands | Partial: local commands work; `/models` and `/compact` explicitly report unavailable live functionality. |
| CHAT-003 | Sessions | Partial: `/new` resets in-memory chat/context without closing the document; persistence pending. |
| CHAT-004 | Live streaming/cancel | Planned: current mock responses are immediate and local. |
| CTX-001 | Selection context | Implemented: selection indicator, explicit attachment, immutable snapshot, stale warning, clear action. |
| AGENT-001 | Built-in agents | Partial: Planner/Blogger metadata/instructions and selection exist; no live inference consumes instructions yet. |
| AGENT-002 | Custom agents | Implemented: YAML-frontmatter Markdown definitions; invalid files warn without dropping valid definitions. |
| SAFE-001 | Proposal separation | Implemented: `/demo` and `/rewrite` create a separate colored proposal; no implicit document mutation. |
| SAFE-002 | Review | Implemented: modal whole-proposal accept/reject, stale revision/original validation, acceptance is undoable and unsaved. |
| SAFE-003 | Save accepted text only | Implemented: a save never includes an unaccepted proposal. |
| SAFE-004 | Agent file permissions | Planned: no agent file tools currently exist; `/ls` and explorer are explicit user actions. |
| CFG-001 | YAML defaults | Partial: embedded defaults plus user fields, schema validation, fallback warnings; CLI overrides pending. |
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

## Release boundary

This is not the full AI MVP. Live provider integration, enforced tool permissions,
credential management, recovery, persistent sessions, full Markdown parsing,
terminal compatibility hardening, and packaging remain required for that release.
