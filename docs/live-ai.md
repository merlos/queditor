# Live AI, privacy, permissions, and recovery

## Multi-provider library

Queditor uses `genai` 0.7 rather than a provider-specific SDK. Its common streaming
and tool-call API supports native OpenAI, Anthropic, Gemini and Ollama protocols,
plus gateways and OpenAI-compatible services. Library support is not a guarantee
that every model supports tools or every provider account has been tested.

Mock mode remains the default. `/models` shows the current model and configured
suggestions. `/models <provider::model>` selects a live model for this run;
`/models mock` returns to offline mode. Use a model available to your account.
There is no automatic account-specific model discovery yet.

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

Set credentials in the environment inherited by `queditor`, using names documented
by genai and your provider. Common names include `OPENAI_API_KEY`,
`ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `GROQ_API_KEY`, and `OPEN_ROUTER_API_KEY`.
Do not put key values in YAML or agent Markdown.

Local Ollama normally needs no key; install/start Ollama separately and download
the desired model before selecting `ollama::<model-name>`.

For a custom **OpenAI-compatible** endpoint, set `GENAI_1_ENDPOINT` to its base URL
(including `/v1/`, where appropriate) and `GENAI_1_API_KEY`. Then select
`/models genai_1::<model-name>`. Different `genai_<number>` namespaces support
multiple endpoints. Use HTTPS remotely; plain HTTP is only appropriate for trusted
local endpoints. Genai controls provider-native authentication and endpoint routing.
The currently pinned genai 0.7.3 custom adapter requires a key value even for
unauthenticated services: use a harmless placeholder only for a trusted local
endpoint that ignores the Authorization header. Native local Ollama needs no key.

API keys are never serialized by Queditor. Raw provider errors/bodies are suppressed
because they can echo prompts or credentials; failures show safe categories. Check
provider/account configuration independently when these diagnostics are insufficient.

## What leaves the machine

A live prompt sends:

- Selected agent instructions and workspace-path metadata.
- Submitted prompt and committed live conversation history.
- An explicitly pinned selection/document snapshot, when present.
- File/list results only after local permission approval.

Selecting text alone does not send it. The full editor document is never attached
implicitly. `/attach clear` removes the current attachment, not previously sent
history. `/new` clears history for future requests; it cannot delete provider-side
data. Switching providers sends committed history to the new provider on the next
prompt. Start a new session if that would be inappropriate.

Provider retention follows provider policy. Queditor requests `store: false` where
supported; this is not a universal no-retention guarantee.

## Edits, cancellation, and limits

Only `propose_edit` can propose a replacement for the explicit editor snapshot.
It never applies/saves changes. Queditor checks original text/revision again after
successful turn completion. Missing context, stale revisions, failure, truncation,
or cancellation never stage/apply generated edits. For live results, press Ctrl+R
to enable acceptance before A/Enter; queued typing cannot accept an unseen proposal.
saving remains separate.

`/cancel` or Ctrl+C in chat stops generation and drops pending tool/proposal events.
Ctrl+C also cancels inside a permission dialog. Partial output remains visibly
marked incomplete/cancelled and is not committed as model history. Cancellation
cannot undo provider processing or earlier transmissions.

Limits: 8 generation rounds, 12 tool calls, 256 KiB attachments/file reads/proposed
replacements, 1000 directory entries, 1 MiB generated/tool context, 30-second
header/stream-read waits, and a five-minute total deadline including permission
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
