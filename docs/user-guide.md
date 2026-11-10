# User guide

## Editor and focus

Start with `queditor [file]`. Type in the editor. Tab switches editor/chat focus.
Arrows move; Shift+arrows select. Ctrl+Left/Right moves by word. Home/End moves to
line boundaries, Ctrl+Home/End to document boundaries, and PageUp/Down by ten lines.
Ctrl+A/E moves to the beginning/end of the current line. Ctrl+K deletes from the
cursor to the end of that line without deleting the newline; the deletion is undoable.
Click to position the cursor, drag to select, and use the wheel to scroll.
Long lines soft-wrap to the editor width without changing the document. Cursor
movement and mouse positioning continue to use the underlying logical line.

Copy/cut/paste editing shortcuts use a local internal clipboard. Terminal paste
is also supported through bracketed paste. Pasted newlines never submit chat.
Undo/redo follows the selected editing profile; `/keys` shows the actual bindings.
Terminal paste also works in `/connect` text and API-key fields. API keys remain
masked; surrounding whitespace and pasted line breaks are discarded.
On the `/connect` provider and credential-source lists, use Up/Down and Enter or
click a row. Editable wizard fields display the terminal cursor, including the
masked API-key field.

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
Shift+Tab cycles agents while chat is focused. Ctrl+G toggles the selected agent
between read-only Chat mode and editing Agent mode; `/mode chat` and `/mode agent`
select explicitly. Each agent's mode, the selected agent, and the model are saved
to `config.yaml` and restored on the next launch.

Select editor text and switch to chat. `Selected line X` or `Selected lines X:Y`
appears above the prompt, and the current selection is automatically snapshotted and sent as important
context with the next live prompt—no additional command is required. The snapshot
can also be the target of a proposed edit. If there is no current selection,
`/attach` pins the selection for later prompts, `/attach document` explicitly pins
all text, and `/attach clear` removes that pinned context. Later edits can make a
pinned snapshot stale. Mock mode sends nothing to a provider. Live mode sends the
selection or pinned attachment and committed history to the selected provider.

`/new` clears active chat/context/temporary grants but keeps the open document.
`/models` lists configured models; `/models <provider::model>` selects and remembers one.
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

Chat mode has no edit tool: it answers questions, critiques text, and can describe
suggested changes without staging them. Agent mode sends a snapshot of the complete
open document and may propose a complete revised document based on the prompt. A
selection is highlighted as important context but does not restrict the editable
range. The result is never applied or saved automatically.

Live results require Ctrl+R to open review, preventing queued typing from accepting
unseen edits. Up/Down selects a changed block, Space includes/excludes that block,
Enter applies all included blocks, A accepts all blocks immediately, and R/Escape
rejects the proposal. Included blocks are validated and applied together as one
undoable edit. You may edit during generation; stale results are rejected.

Escape cancels active agent work from either pane, including thinking, streaming,
tool work, provider waits, and permission waits. The chat title animates and reports
the active state while work is in progress.

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
