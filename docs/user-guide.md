# User guide

## Editor and focus

Start with `queditor [file]`. Type in the editor. Tab switches editor/chat focus.
Arrows move; Shift+arrows select. Ctrl+Left/Right moves by word. Home/End moves to
line boundaries, Ctrl+Home/End to document boundaries, and PageUp/Down by ten lines.
Click to position the cursor, drag to select, and use the wheel to scroll.
The editor scrolls horizontally rather than wrapping long lines.

Copy/cut/paste editing shortcuts use a local internal clipboard. Terminal paste
is also supported through bracketed paste. Pasted newlines never submit chat.
Undo/redo follows the selected editing profile; `/keys` shows the actual bindings.

At widths >=124 columns, panels have at least 80 and 40 usable columns. Below
that, the editor sits above chat. Terminals smaller than 30×10 show a resize message.

## Files

- Ctrl+S or `/save` saves the current document.
- `/save "new name.md"` saves to an unused path; existing unrelated files are not overwritten.
- `/open "file.md"` opens text, or creates an empty document at a missing filename.
- `/files [directory]` opens the explorer in the chat pane.
- Explorer arrows choose an entry; Enter opens it; Escape returns to chat.
- `/cd <directory>` changes the chat workspace, not the open document's path.
- `/ls [directory]` prints a user-requested listing in chat.

Paths containing spaces need quotes. Relative command paths use the chat workspace.
`~/` expands to your home directory. Missing parent directories are not created.

Opening another file or exiting with dirty content displays a save/discard/cancel
confirmation. For an unnamed file, choose save, then enter `/save <path>` and retry.
Existing symlinks resolve to their targets. NUL-containing or non-UTF-8 files are
rejected, as are files larger than 8 MiB.

External changes are checked when saving. If a file changed or disappeared, save
to a new filename or cancel/reopen. Private recovery checkpoints are enabled by
default; `/recover` lists snapshots and `/recover <id>` restores into the editor
without writing the document. Checkpoints are not a substitute for explicit saves.

## Search

In chat, `/find "text to find"` selects the next match, wrapping to the start.
`/replace "old text" "new text"` replaces all occurrences in one undoable edit.
Use `""` as the replacement to delete occurrences.

## Chat and context

Enter submits. Ctrl+J inserts a newline; Shift+Enter works in terminals that
distinguish it. Typing `/` shows suggestions: arrows select, Tab completes.
Escape clears unsent input. PageUp/PageDown or the wheel scroll chat. Scrolling
away from the bottom stops auto-follow until you return to the bottom.
Shift+Tab cycles agents while chat is focused.

Select editor text, switch to chat, and run `/attach`. This pins a snapshot and
shows its byte count. `/attach document` explicitly attaches all text; `/attach
clear` removes it. Later edits make the snapshot stale; reattach before using it
for a proposal or live prompt. Mock mode sends nothing to a provider. Live mode
sends explicitly attached text and committed history to the selected provider.

`/new` clears active chat/context/temporary grants but keeps the open document.
`/models` lists configured models; `/models <provider::model>` selects one.
`/models mock` returns to offline mode. `/compact` summarizes committed live
history while retaining the visible transcript. `/cancel` stops an active request.
Optional persistence and recovery are described in [live AI](live-ai.md).

## Review demonstration

`/demo` proposes uppercase text for attached context, current selection, or the
whole document, in that priority order. `/rewrite "replacement"` proposes the
supplied text. These are deterministic local demonstrations, not model output.

The editor displays a diff with `-` deletions/red and `+` additions/green. During
review, ordinary editing is paused. A or Enter accepts; R or Escape rejects.
Use arrows/PageUp/PageDown to scroll the diff. Acceptance is not a save and can be
undone. Saving while a proposal exists saves only the unchanged accepted document.

On quitting/opening during review, saving-and-continuing saves accepted text and
discards the proposal. Cancel the confirmation to return to review.

## Live edits and file permissions

Live edits require an explicit `/attach` snapshot. Ask the agent to revise it;
`propose_edit` stages a replacement only after successful turn completion. A/R
review and undo work as in the mock demonstration, but live results require Ctrl+R
to enable acceptance first (preventing queued typing from accepting unseen edits).
You may edit during generation;
stale results are rejected.

`read_file` and `list_directory` show a permission dialog with the canonical path
and operation. O allows once, S for the session, A persistently for that exact
path/operation/workspace, and N/Escape denies. Approval sends the result to the
provider. Sensitive paths remain excluded. `/permissions` inspects grants;
`/permissions clear` revokes all. `/new`, `/cd`, and `/resume` clear temporary grants.
No agent shell execution, writes, or deletion are available.

## Troubleshooting

- Use `queditor --check` to see startup settings and configuration warnings.
- If Shift+Enter submits instead of inserting a newline, use Ctrl+J.
- If a terminal reserves a shortcut, use commands or an alternate profile binding.
- Terminal raw mode, mouse capture, and alternate-screen state are restored on
  normal exit, ordinary errors, unwinding panics, and handled Unix shutdown signals.
  SIGKILL cannot guarantee cleanup; recovery only includes the latest checkpoint.
