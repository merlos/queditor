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
mode exposes `propose_edit` for a complete replacement of the open document. It
never directly applies or saves changes. Queditor checks original text/revision
again after successful turn completion. Stale revisions, failure, truncation, or
cancellation never stage/apply generated edits.

For live results, Ctrl+R opens review before acceptance keys are active, so queued
typing cannot accept an unseen proposal. Up/Down navigates diff blocks, Space
includes/excludes one, Enter applies included blocks, A applies all, and R/Escape
rejects all. Partial choices are composed against the original snapshot and applied
as one validated, undoable edit. Saving remains separate.

Escape from either pane always stops active generation, streaming, tool execution,
provider waits, or permission waits and drops pending tool/proposal events. `/cancel`
and Ctrl+C in chat remain alternatives. Partial output remains visibly
marked incomplete/cancelled and is not committed as model history. Cancellation
cannot undo provider processing or earlier transmissions.

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

- `recovery-<instance-id>.json`: accepted unsaved text, cursor, path and save baseline.
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
document checkpoints, without deleting existing records.

Session persistence defaults **off** because history can contain approved file
contents and private prompts. `persist_sessions: true` enables checkpoints and
saving on exit or `/new`. `/sessions` lists IDs; `/resume <id>` restores history as
a new session without changing the editor document. Temporary grants/current
attachments are not restored. Restored history may be sent to its model on the
next prompt. `/sessions delete <id>` deletes an old session; start `/new` before
deleting a session still being checkpointed. Undo history and pending proposals
are not persisted.
