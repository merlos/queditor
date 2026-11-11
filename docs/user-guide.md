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
The editor and prompt use lighter, borderless background panels. Submitted messages
use the same lighter background; agent replies stay on the standard background.
The filename and unsaved-change marker appear in the bottom status line before
the line/column numbers; the editor and assistant headers are empty. Messages are labeled `You:` or with the
responding agent's name (for example, `Planner:`); changing agents does not rename
earlier responses. `You:` uses the theme's green color, while agent names use the
accent color. The agent name below the prompt matches its chat heading; the model
label and Chat/Agent mode stay muted. Running activity and a full-cycle spinner appear directly below
the responding agent's name, together with the Escape cancellation hint. The prompt
panel includes the selected agent, mode, and model, for example `Planner Chat · provider:model:25b`.
The spinner smoothly cycles through rainbow colors; activity text stays muted.
A blank row separates the prompt text from this label when space permits. In the
side-by-side layout, the prompt panel's bottom edge aligns with the editor panel.
Completed replies end with the model used for that response; changing models does
not relabel earlier replies. Older saved replies without model metadata remain
unattributed. Mock responses and the mock prompt label explicitly say `mock (offline)`.
The bottom bar shows the filename, unsaved-change marker, and cursor position,
without focus, agent, model, or theme names.
It also shows Unicode word and character counts (characters are grapheme clusters),
the live document's UTF-8 size, and total input/output tokens observed during this
application run. Size uses decimal units: b below 1,000 bytes, then KB, MB, and GB.
Counts include unsaved text, not mock previews or deleted diff rows; the bar wraps
on narrow screens so these statistics remain visible.

Completed live replies show `in <tokens> out <tokens> · <rate> tk/s` beside their
original model. Counts come from the provider and include all tool-generation rounds;
speed is output tokens divided by combined provider request time, including network
latency and reasoning but excluding local tools and permission waits. `?` means
unreported usage or unavailable speed; `123+?` means at least 123 known tokens with
additional usage unknown. Cancelled/failed requests retain reported rounds but may
have unreported consumption. Mock replies stay explicitly offline without invented
usage. `/new` does not reset run totals; resuming history does not count old usage
again. Per-response metrics are saved only when session persistence is enabled.

Bot replies use Markdown colors for headings, bold/italic emphasis, inline and
fenced code, links/images, block quotes, and strikethrough, including while streaming.
Markdown markers remain visible, and long lines wrap without losing their colors.
Submitted messages and system output stay plain. Links and images are only colored
text: the renderer does not open URLs or fetch content. Code blocks use a code color,
not language-specific syntax highlighting.

## Files

- Ctrl+S or `/save` saves the current document.
- For an unnamed document, Ctrl+S, `/save`, or `/sa` then Enter opens a filename
  popup. Type or paste a path (spaces need no quotes here); Enter saves and Escape
  cancels. Relative paths use the chat workspace. Save errors stay in the popup
  so you can correct the filename.
- `/save "new name.md"` saves to an unused path; existing unrelated files are not overwritten.
- `/open "file.md"` opens text, or creates an empty document at a missing filename.
- `/files [directory]` opens the explorer in the chat pane.
- Explorer arrows choose an entry; Enter opens it; Escape returns to chat.
- `/cd <directory>` changes the chat workspace, not the open document's path.
- `/ls [directory]` prints a user-requested listing in chat.

Paths containing spaces need quotes. Relative command paths use the chat workspace.
`~/` expands to your home directory. Missing parent directories are not created.

Opening another file or exiting with dirty content displays a save/discard/cancel
confirmation. For an unnamed file, choosing save opens the filename popup; saving
successfully continues the original operation. Escape returns to the confirmation.
Popup questions and answers use distinct colors, with one answer per line. Use
Up/Down and Enter, click an answer, or press its displayed shortcut. Escape cancels
where shown. This interaction is consistent across confirmations, permissions,
provider setup, credential warnings, model selection, and command suggestions.
Existing symlinks resolve to their targets. NUL-containing or non-UTF-8 files are
rejected, as are files larger than 8 MiB.

External changes are checked when saving. If a file changed or disappeared, save
to a new filename or cancel/reopen. Private recovery checkpoints are enabled by
default; `/recover` opens a newest-first snapshot picker showing a short identifier,
age, and document path (or `[Untitled]`). Use Up/Down and Enter or click a snapshot;
Escape closes the picker. Selecting a snapshot or using `/recover <id>` restores
into the editor without writing the document, after any unsaved-change confirmation.
`/recover delete <id>` deletes a snapshot. Checkpoints are not a substitute for explicit saves.

## Search

In chat, `/find "text to find"` selects the next match, wrapping to the start.
`/replace "old text" "new text"` replaces all occurrences in one undoable edit.
Use `""` as the replacement to delete occurrences.

## Chat and context

Enter submits. Ctrl+J inserts a newline; Shift+Enter works in terminals that
distinguish it. Typing `/` shows suggestions: arrows select, Enter completes and
runs the highlighted command, and Tab completes without submitting so you can add
arguments. For example, `/co` then Enter opens `/connect` immediately.
Slash commands are not echoed as `You:` messages in either mode; command output
and errors remain visible, and successful commands can still be recalled with Up.
`/help` lists commands; `/version` displays the running Queditor version (for example,
`queditor 0.1.0`), the same package version reported by `queditor --version`.
Command errors appear in red directly above the prompt hints; failed input stays
in the prompt for correction. Long errors wrap across up to three lines.
Long prompt lines wrap within the composer. Up on the prompt's first visual line
recalls older submitted prompts; Down on its last visual line moves toward newer
prompts and eventually restores the unsent draft. Within a multiline or wrapped
prompt, Up and Down move between visual lines instead.
Ctrl+A and Ctrl+E move to the start and end of the current logical prompt line.
Ctrl+K deletes from the cursor to that line's end, preserving its newline and the
following lines. These shortcuts work in both editing profiles and in the editor;
soft wrapping does not change which logical line they operate on.
Escape clears unsent input. PageUp/PageDown or the wheel scroll chat. Scrolling
away from the bottom stops auto-follow until you return to the bottom.
Shift+Tab cycles agent/mode pairs from either pane without changing focus: Blogger
Chat → Blogger Agent → Planner Chat → Planner Agent → Blogger Chat with the two
built-in agents. All loaded custom agents join in agent-list order. Each agent's
modes are visited in order (currently Chat, then Agent), starting at Chat when
moving to another agent even if that agent previously remembered Agent mode.
Ctrl+G cycles only the selected agent's mode; `/mode chat` and `/mode agent`
select explicitly. Rotation saves the agent and mode together; if saving fails,
neither changes. Each agent's mode, the selected agent, and the model are saved
to `config.yaml` and restored on the next launch.
`/agents` then Enter opens an agent picker with names and descriptions. Use Up/Down
and Enter, or click a row, to select and remember an agent. Escape closes without
changing the agent. `/agents <name>` selects directly without opening the popup.

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

## Offline review demonstration

`/demo` proposes uppercase text for attached context, current selection, or the
whole document, in that priority order. `/rewrite "replacement"` proposes the
supplied text. These are deterministic local demonstrations, not model output.

The editor remains usable while mock changes are pending. It previews the proposed
document directly in the editor, with proposed replacement text highlighted. The
bottom review line keeps the acceptance controls visible. Alt+] and Alt+[ move
between changes, Ctrl+Enter keeps the active change, and Ctrl+Backspace rejects it.
These modified keys require terminal enhanced-keyboard support; `/changes keep` and
`/changes reject` are portable alternatives. `/changes keep-all` and `/changes
reject-all` resolve the complete proposal. Ordinary typing, Enter, and Escape never
accept changes. Acceptance is not a save and can be undone.

If you edit the document while changes are pending, Queditor preserves your edit and
closes the now-stale review layer rather than overwriting your work. Saving while a
proposal exists saves only the unchanged accepted document.

On quitting/opening during review, saving-and-continuing saves accepted text and
discards the proposal. Cancel the confirmation to return to review.

## Live edits and file permissions

Chat mode has no edit tool and never changes files. Agent mode sends a snapshot of
the complete open document; selections are important context, not edit boundaries.
**Live Agent edits apply and save automatically before review**, after successful
completion and revision/original-content validation. Unnamed documents automatically
use a private temporary backing file while staying “[Untitled]”. Ctrl+S still asks
for a real filename; after Save As, Agent edits and review Undo continue on that file.
Temporary backing is cleaned up on close; enabled recovery retains pre-edit snapshots
and unsaved untitled text checkpoints for explicit restoration after interruptions.
The automatic save includes the whole file, including existing unsaved text. Editing
during generation is allowed, but makes an older generated result stale and unapplied.
External disk changes and failed saves leave the editor unchanged.

The editable unified diff shows old/new line numbers, red `−` removals, green `+`
additions, and stronger word-level highlights. Deleted lines and diff headers are
review-only; clicking live added/context lines positions the normal editing cursor.
Moving the cursor into a pending change selects and highlights that block. Clicking
a removed line or a block's change header also selects it without moving the text
cursor. Click its `Ctrl+Enter Keep` or `Ctrl+Backspace Undo` label to act directly
on that block; no modifier key is needed. Moving into unchanged context retains
the last selected block, and the keyboard shortcuts act on the highlighted block.
Long lines wrap with aligned continuation rows. Full context is shown by default;
`/changes collapse` shows three context lines per diff group, and `/changes expand`
restores the full document. Keyboard navigation into hidden context expands it.

- Alt+[ / Alt+] selects a change.
- Ctrl+Enter or `/changes keep` confirms it; nothing is applied or saved by Keep.
- Ctrl+Backspace or `/changes undo` reverts it and saves the reversal immediately.
- `/changes keep-all` and `/changes undo-all` resolve all remaining changes.
- `reject` and `reject-all` remain aliases for Undo, for compatibility.

The next pending change is selected after a decision. Kept/undone changes lose
their diff highlights. Typing edits the current live document but is not auto-saved;
Ctrl+S saves it normally. Keep does not save later manual edits. Review Undo also
saves any remaining manual text as part of saving the whole file. Edits outside an
agent change shift its review positions; edits that touch it protect that change
from review Undo, so your work is never blindly replaced. Ordinary Ctrl+Z undo/redo
remains an unsaved editor operation, separate from review Undo.

With recovery enabled, a private pre-edit checkpoint is written before automatic
saving and survives Keep and normal exit. `/recover` can restore it into the editor
for inspection; saving remains explicit. Recovery storage failure prevents auto-apply
unless you explicitly disable recovery. No review state is restored across restarts.

Pending live edits are already on disk: previewers and synchronization tools can see
them before Keep. Review before publishing; Undo cannot reverse external effects.
Quitting/opening during review does not revert saved edits. “Discard” discards only
unsaved text; use review Undo first if you want to reverse saved agent edits.

Escape cancels active agent work from either pane, including thinking, streaming,
tool work, provider waits, and permission waits. Activity and a spinner appear below
the responding agent's name. Escape does not revert edits already applied and saved.

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
