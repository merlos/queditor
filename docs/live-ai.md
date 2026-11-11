# Live AI, privacy, permissions, and recovery

## Multi-provider library

Queditor uses `genai` 0.7 rather than a provider-specific SDK. Its common streaming
and tool-call API supports native OpenAI, Anthropic, Gemini and Ollama protocols,
plus gateways and OpenAI-compatible services. Library support is not a guarantee
that every model supports tools or every provider account has been tested.

Mock mode remains the default. `/connect` provides guided provider setup and model
discovery. `/models` opens a selector containing configured and discovered models.
`/models <provider::model>` also selects and remembers a live model;
`/models mock` returns to offline mode. Use a model available to your account.
Discovery uses the provider's model-list API and can fail even when manual model use
works; in that case enter the documented model ID directly.

Startup configuration example:

```yaml
model: openai::gpt-4o-mini
models:
  - mock
  - openai::gpt-4o-mini
  - anthropic::claude-sonnet-4-5
  - gemini::gemini-2.5-flash
  - ollama::llama3.2
persist_sessions: false
recovery: true
```

## Credentials and endpoints

Run `/connect` and choose OpenAI, Anthropic, Gemini, OpenRouter, or Custom
OpenAI-compatible. Credential choices are the OS keyring, an environment-variable
reference, or a private plaintext fallback. API-key input is masked and never enters
chat, session, or recovery history.

Environment credentials use names inherited by `queditor`. Common names include
`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `GROQ_API_KEY`, and
`OPEN_ROUTER_API_KEY`.
Do not put key values in `config.yaml` or agent Markdown. `/connect` stores only the
environment-variable/keyring/file reference there. Keyring is preferred. The
`credentials.yaml` fallback is mode 0600 on Unix but remains unencrypted plaintext.

Local Ollama normally needs no key; install/start Ollama separately and download
the desired model before selecting `ollama::<model-name>`.

For a custom **OpenAI-compatible** endpoint, `/connect` asks for its base URL and
discovers `GET <base>/models`; if unavailable, enter `/models <id>::<model>` manually.
Remote endpoints require HTTPS; plain HTTP is limited to loopback. URLs containing
credentials, queries, or fragments are rejected, and authenticated redirects are
disabled to keep bearer keys on the configured origin.

The older compatibility path remains available: set `GENAI_1_ENDPOINT` to its base URL
(including `/v1/`, where appropriate) and `GENAI_1_API_KEY`. Then select
`/models genai_1::<model-name>`. Different `genai_<number>` namespaces support
multiple endpoints. Use HTTPS remotely; plain HTTP is only appropriate for trusted
local endpoints. Genai controls provider-native authentication and endpoint routing.
The currently pinned genai 0.7.3 custom adapter requires a key value even for
unauthenticated services: use a harmless placeholder only for a trusted local
endpoint that ignores the Authorization header. Native local Ollama needs no key.

API keys are never serialized in ordinary config, sessions, or recovery. A key is
serialized only when the user explicitly selects the private plaintext fallback.
Raw provider errors/bodies are suppressed
because they can echo prompts or credentials; failures show safe categories. Check
provider/account configuration independently when these diagnostics are insufficient.

## What leaves the machine

A live prompt sends:

- Selected agent instructions and workspace-path metadata.
- Submitted prompt and committed live conversation history.
- In Chat mode, the current editor selection (automatically snapshotted) or an
  explicitly pinned snapshot when no current selection exists.
- In Agent mode, the complete open document; a selection or pinned snapshot is also
  identified as especially important prompt context.
- File/list results only after local permission approval.

Selecting text marks it as important context and sends it with the next live prompt;
the chat pane shows `Selected line X` for one line or the inclusive one-based range
`Selected lines X:Y` for multiple lines. Agent mode additionally sends the full
document so it can edit freely; Chat mode does not. `/attach clear` removes the pinned attachment, not a current
selection or previously sent history. `/new` clears history for future requests; it cannot delete provider-side
data. Switching providers sends committed history to the new provider on the next
prompt. Start a new session if that would be inappropriate.

Provider retention follows provider policy. Queditor requests `store: false` where
supported; this is not a universal no-retention guarantee.

## Edits, cancellation, and limits

Every agent has a remembered mode. Chat mode is read-only and does not expose
`propose_edit`; it can answer, critique, and describe suggested revisions. Agent
mode exposes `propose_edit` for a complete replacement of the open document.
**On successful completion, Queditor validates the snapshot, applies the edit and
saves the whole file before review**, including existing unsaved text. Unnamed
documents use private temporary backing without changing their “[Untitled]” identity.
Ctrl+S or `/save` opens Save As; once saved to a real filename, subsequent edits and
review Undo use that file. Temporary backing is cleaned up when the document closes.
With recovery enabled, untitled text is still checkpointed for explicit restoration.
Stale revisions, failure,
truncation, cancellation, and failed disk-baseline/save checks never apply generated edits.
Ordinary assistant text cannot change files; there are no arbitrary write tools.

The unified inline diff remains editable and shows both line-number columns, red
removals, green additions, and word-level highlights. Alt+] and Alt+[ navigate changes;
Ctrl+Enter or `/changes keep` clears a change's pending review state. Ctrl+Backspace
or `/changes undo` reverts and saves that change. Use `keep-all` or `undo-all` for all
pending changes; `reject`/`reject-all` are aliases for Undo. Full context is the default;
`/changes collapse` and `/changes expand` toggle three-line context/full document.
Removed lines and diff headers have no editable cursor position.
Moving the cursor into a change selects and highlights it; clicking a removed line
or change header selects its block without moving the text cursor. Clicking a
block's `Ctrl+Enter Keep` or `Ctrl+Backspace Undo` label acts on that block directly,
without holding Ctrl. In unchanged context the last selected block stays active.

Manual edits outside a pending change rebase its positions. Touching a pending change
protects it from review Undo; other changes remain reviewable. Ordinary typing and
editor undo/redo are unsaved until Ctrl+S. Keep does not save later manual text, but
review Undo saves the complete current file, including preserved manual text. An
external disk change blocks Undo rather than overwriting it. Initial application
and an all-change reversal each form one undoable editor transaction.

Pending edits are already on disk and visible to external preview, sync, or publishing
tools. Review before publishing; Undo cannot reverse external effects. Leaving review
or exiting does not revert saved edits. Offline `/demo` and `/rewrite` still produce
unsaved mock previews with explicit acceptance; they do not invoke a model.

Escape from either pane always stops active generation, streaming, tool execution,
provider waits, or permission waits and drops pending tool/proposal events. `/cancel`
and Ctrl+C in chat remain alternatives. Partial output remains visibly
marked incomplete/cancelled and is not committed as model history. Cancellation
cannot undo provider processing or earlier transmissions, or revert a completed saved
edit. Use review Undo for pending saved changes.

Limits: 8 generation rounds, 12 tool calls, 256 KiB attachments/open documents/file
reads/proposed replacements, 1000 directory entries, 1 MiB generated/tool context, 120-second
header/stream-read waits, and a ten-minute total deadline including permission
waits. No automatic retries, avoiding duplicate actions/billing.

`/compact` summarizes committed history without tools, replacing model-facing
history only after successful nonempty completion. Visible chat remains. Summaries
can omit details: inspect the result. Compaction accepts up to 4 MiB of local
history; ordinary prompts are limited to 1 MiB.

## Permissions

Every agent read/list is ask-by-default, including inside the workspace. The dialog
shows canonical path, exact operation, and an outside-workspace warning if needed.

- O: once.
- S: session, exact path/operation/workspace.
- A: saved, exact path/operation/workspace, not a directory tree.
- N/Escape: deny; the model receives a denial, not contents.

`/permissions` lists grants. `/permissions clear` revokes all saved/temporary grants.
`/new`, `/cd`, and `/resume` clear temporary grants. Saved grants remain tied to
their original workspace. There are no shell, deletion, or direct write tools.

Sensitive paths such as `.env*`, `.ssh`, `.gnupg`, `.aws`, `.azure`, `.kube`, `.git`,
credential files and private-key extensions are excluded. Canonicalization checks
symlink targets; capabilities stay anchored across permission waits. Reads check
file identity and UTF-8/size limits after approval. These are application controls,
not a complete OS sandbox or a guarantee innocuously named files contain no secrets.

## Local state and recovery

On Linux/macOS, state lives in `$XDG_STATE_HOME/queditor/` or
`~/.local/state/queditor/`. On Windows it lives in `%LOCALAPPDATA%\queditor`:

- `recovery-<instance-id>.json`: unsaved text, cursor, path and save baseline.
- Additional `recovery-<uuid>.json` records: independent pre-agent-edit snapshots,
  retained after Keep and clean exit. With recovery enabled, a checkpoint must be
  written before automatic save; storage failure blocks application. The saved-file
  baseline is refreshed after success so restoring pre-edit text can be saved back
  if the file has not subsequently changed. Later disk changes still block overwrite.
- `session-<session-id>.json`: transcript, committed history, model/agent and workspace.
- `permissions.json`: explicitly persistent grants only.

On Unix the directory is mode 0700 and records are 0600. State is private plaintext,
**not encrypted**. Recovery defaults on and checkpoints roughly once per second
after activity, never writing the original document. Uncheckpointed input can be
lost; storage failures warn. Clean exit removes the current recovery record;
explicit discard is intentional. Old crash records are not silently deleted.
On Unix, SIGTERM/SIGHUP/SIGINT trigger a final checkpoint and terminal restoration,
retaining recovery rather than treating interruption as an intentional discard.
SIGKILL cannot run cleanup and may lose input since the last checkpoint.

`/recover` lists IDs. `/recover <id>` restores into the editor after dirty-content
confirmation. Inspect and save explicitly; external-change checks still apply.
`/recover delete <id>` deletes an old snapshot. `recovery: false` disables new
document checkpoints (including pre-agent-edit snapshots), without deleting existing
records. In-memory undo remains available, but disabling recovery removes the durable
pre-edit safeguard. Pre-edit records may contain sensitive text; delete them explicitly
when no longer needed. Recovery restores into the editor, never directly to disk.

Session persistence defaults **off** because history can contain approved file
contents and private prompts. `persist_sessions: true` enables checkpoints and
saving on exit or `/new`. `/sessions` lists IDs; `/resume <id>` restores history as
a new session without changing the editor document. Temporary grants/current
attachments are not restored. Restored history may be sent to its model on the
next prompt. `/sessions delete <id>` deletes an old session; start `/new` before
deleting a session still being checkpointed. Undo history and pending proposals
are not persisted. Already-saved agent changes remain in their files; reopening a
file does not recreate its pending review highlights.
