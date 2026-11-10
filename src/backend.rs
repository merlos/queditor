//! Provider-independent streaming/tool loop powered by genai. No terminal I/O here.
use crate::tools::{self, MAX_CONTEXT_BYTES, Target};
use anyhow::{Result, bail};
use futures::StreamExt;
use genai::{
    Client, WebConfig,
    chat::{ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent, Tool, ToolResponse},
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio::sync::{mpsc, oneshot};

pub struct Request {
    pub model: String,
    pub system: String,
    pub history: Vec<ChatMessage>,
    pub workspace: PathBuf,
    pub compact: bool,
    pub allow_edits: bool,
}
#[derive(Debug)]
pub enum Event {
    Text(String),
    Permission {
        target: Box<Target>,
        reply: oneshot::Sender<Option<Target>>,
    },
    Proposal(String),
    Done(Vec<ChatMessage>),
    Failed(String),
}

pub fn client() -> genai::Result<Client> {
    Client::builder()
        .with_web_config(WebConfig::default().with_connect_timeout(Duration::from_secs(15)))
        .build()
}

pub fn definitions() -> Vec<Tool> {
    vec![
        Tool::new("read_file").with_description("Read UTF-8 text after user permission. Sensitive files are prohibited.")
            .with_schema(json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})),
        Tool::new("list_directory").with_description("List a directory after user permission.")
            .with_schema(json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})),
        Tool::new("propose_edit").with_description("Propose replacement for the explicitly attached editor snapshot only. Never saves or applies changes.")
            .with_schema(json!({"type":"object","properties":{"replacement":{"type":"string"}},"required":["replacement"],"additionalProperties":false})),
    ]
}

/// Only library error categories are exposed: provider bodies can echo credentials/prompts.
pub fn safe_error(error: &genai::Error) -> String {
    match error {
        genai::Error::HttpError { status, .. } => format!("Provider returned HTTP {status}; check credentials, model access, and rate limits"),
        genai::Error::RequiresApiKey { .. } | genai::Error::NoAuthData { .. } | genai::Error::Resolver { .. } => "Provider authentication/resolution failed; check the provider's API-key environment variable".into(),
        _ => "Provider request/stream failed; check model, endpoint, credentials, and network. Sensitive provider diagnostics are suppressed.".into(),
    }
}

pub async fn run(request: Request, sender: mpsc::Sender<Event>) {
    let result =
        tokio::time::timeout(Duration::from_secs(300), conversation(request, &sender)).await;
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e.to_string(),
        Err(_) => "Agent request timed out after five minutes (including permission waits)".into(),
    };
    let _ = sender.send(Event::Failed(error)).await;
}

async fn conversation(request: Request, sender: &mpsc::Sender<Event>) -> Result<()> {
    let client = client().map_err(|e| anyhow::anyhow!(safe_error(&e)))?;
    let mut chat = ChatRequest::new(request.history)
        .with_system(request.system)
        .with_store(false);
    if !request.compact {
        chat = chat.with_tools(
            definitions()
                .into_iter()
                .filter(|t| request.allow_edits || t.name.as_str() != "propose_edit"),
        );
    }
    let options = ChatOptions::default()
        .with_max_tokens(8192)
        .with_capture_content(true)
        .with_capture_tool_calls(true)
        .with_capture_reasoning_content(true)
        .with_stream_header_timeout(Duration::from_secs(30))
        .with_stream_read_timeout(Duration::from_secs(30));
    let mut total = 0;
    let mut tool_count = 0;
    let mut proposed = false;
    for _ in 0..8 {
        let mut stream = client
            .exec_chat_stream(&request.model, chat.clone(), Some(&options))
            .await
            .map_err(|e| anyhow::anyhow!(safe_error(&e)))?
            .stream;
        let mut end = None;
        while let Some(event) = stream.next().await {
            match event.map_err(|e| anyhow::anyhow!(safe_error(&e)))? {
                ChatStreamEvent::Chunk(chunk) => {
                    total += chunk.content.len();
                    if total > 1024 * 1024 {
                        bail!("Agent response exceeds 1 MiB");
                    }
                    sender.send(Event::Text(chunk.content)).await?;
                }
                ChatStreamEvent::ToolCallChunk(chunk) => {
                    if chunk.tool_call.size() > MAX_CONTEXT_BYTES {
                        bail!("Tool call exceeds size limit");
                    }
                }
                ChatStreamEvent::ReasoningChunk(chunk) => {
                    total += chunk.content.len();
                    if total > 1024 * 1024 {
                        bail!("Agent reasoning/response exceeds 1 MiB");
                    }
                }
                ChatStreamEvent::End(value) => {
                    end = Some(value);
                    break;
                }
                _ => {}
            }
        }
        let end = end.ok_or_else(|| {
            anyhow::anyhow!("Provider stream ended without a completion marker; no edits staged")
        })?;
        if end
            .captured_stop_reason
            .as_ref()
            .is_some_and(|r| r.is_max_tokens())
        {
            bail!("Provider response was truncated; no edits staged");
        }
        let calls = end
            .captured_tool_calls()
            .unwrap_or_default()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        if calls.is_empty() {
            let content = end.captured_content.ok_or_else(|| {
                anyhow::anyhow!("Provider returned no captured assistant content")
            })?;
            chat = chat.append_message(
                ChatMessage::assistant(content)
                    .with_reasoning_content(end.captured_reasoning_content),
            );
            sender.send(Event::Done(chat.messages)).await?;
            return Ok(());
        }
        if request.compact {
            bail!("Provider attempted tool calls during compaction; original history retained");
        }
        let assistant = end
            .into_assistant_message_for_tool_use()
            .ok_or_else(|| anyhow::anyhow!("Provider returned an invalid tool handoff"))?;
        chat = chat.append_message(assistant);
        for call in calls {
            if call.size() > MAX_CONTEXT_BYTES {
                bail!("Tool call exceeds 256 KiB");
            }
            tool_count += 1;
            if tool_count > 12 {
                bail!("Agent exceeded the 12-tool-call limit");
            }
            let result = if call.fn_name == "propose_edit" {
                if !request.allow_edits {
                    Err(anyhow::anyhow!(
                        "An explicit editor attachment is required for proposed edits"
                    ))
                } else if proposed {
                    Err(anyhow::anyhow!("Only one proposal is allowed per turn"))
                } else {
                    let replacement = string_arg(&call.fn_arguments, "replacement")?;
                    if replacement.len() > MAX_CONTEXT_BYTES {
                        bail!("Proposed edit exceeds 256 KiB");
                    }
                    sender.send(Event::Proposal(replacement.into())).await?;
                    proposed = true;
                    Ok("Proposal queued for explicit review after successful completion; not applied or saved.".into())
                }
            } else if matches!(call.fn_name.as_str(), "read_file" | "list_directory") {
                let path = string_arg(&call.fn_arguments, "path")?.to_owned();
                let workspace = request.workspace.clone();
                let op = call.fn_name.clone();
                match tokio::task::spawn_blocking(move || tools::prepare(&workspace, &path, &op))
                    .await?
                {
                    Ok(target) => {
                        // Move the anchored capability through the UI and back without reading it.
                        let (reply, decision) = oneshot::channel();
                        sender
                            .send(Event::Permission {
                                target: Box::new(target),
                                reply,
                            })
                            .await?;
                        if let Some(target) = decision.await.unwrap_or(None) {
                            tokio::task::spawn_blocking(move || tools::execute(target)).await?
                        } else {
                            Err(anyhow::anyhow!("User denied file access"))
                        }
                    }
                    Err(e) => Err(e),
                }
            } else {
                Err(anyhow::anyhow!("Unknown or prohibited tool"))
            };
            let content = match result {
                Ok(text) => json!({"result":text}),
                Err(e) => json!({"error":e.to_string()}),
            }
            .to_string();
            total += content.len();
            if total > 1024 * 1024 {
                bail!("Turn context exceeds 1 MiB; attach less text or compact");
            }
            chat = chat.append_message(ChatMessage::from(ToolResponse::from_tool_call(
                &call, content,
            )));
        }
    }
    bail!("Agent exceeded the eight-generation limit")
}

fn string_arg<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Tool arguments must be an object"))?;
    if object.len() != 1 {
        bail!("Unexpected tool arguments");
    }
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Missing string tool argument"))
}
