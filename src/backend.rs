//! Provider-independent streaming/tool loop powered by genai. No terminal I/O here.
use crate::tools::{self, MAX_CONTEXT_BYTES, Target};
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use genai::{
    Client, WebConfig,
    chat::{ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent, Tool, ToolResponse, Usage},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const STREAM_TIMEOUT: Duration = Duration::from_secs(1800);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1800);
const MAX_GENERATIONS: usize = 100;
const MAX_TOOL_CALLS: usize = 100;

#[derive(Default)]
struct TurnBudget {
    generations: usize,
    tool_calls: usize,
}

impl TurnBudget {
    fn next_generation(&mut self) -> Result<()> {
        if self.generations >= MAX_GENERATIONS {
            bail!("Agent exceeded the {MAX_GENERATIONS}-generation limit; no edits applied");
        }
        self.generations += 1;
        Ok(())
    }

    fn record_tool_call(&mut self) -> Result<()> {
        if self.tool_calls >= MAX_TOOL_CALLS {
            bail!("Agent exceeded the {MAX_TOOL_CALLS}-tool-call limit; no edits applied");
        }
        self.tool_calls += 1;
        Ok(())
    }
}

pub struct Request {
    pub model: String,
    pub provider: Option<crate::config::Provider>,
    pub system: String,
    pub history: Vec<ChatMessage>,
    pub workspace: PathBuf,
    pub compact: bool,
    pub allow_edits: bool,
}
#[derive(Debug)]
pub enum Event {
    Text(String),
    RequestStarted,
    Usage(RequestUsage),
    Permission {
        target: Box<Target>,
        reply: oneshot::Sender<Option<Target>>,
    },
    Proposal(String),
    Done(Vec<ChatMessage>),
    Failed(String),
}

/// Provider-reported totals across tool rounds. Missing counts are never estimated.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RequestUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub input_unknown: bool,
    pub output_unknown: bool,
    pub seconds: f64,
    pub rounds: u64,
}

impl RequestUsage {
    pub fn from_provider(usage: Option<&Usage>, duration: Duration) -> Self {
        let input = usage
            .and_then(|u| u.prompt_tokens)
            .and_then(|n| u64::try_from(n).ok());
        let output = usage
            .and_then(|u| u.completion_tokens)
            .and_then(|n| u64::try_from(n).ok());
        Self {
            input_tokens: input.unwrap_or(0),
            output_tokens: output.unwrap_or(0),
            input_unknown: input.is_none(),
            output_unknown: output.is_none(),
            seconds: duration.as_secs_f64(),
            rounds: 1,
        }
    }

    pub fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.input_unknown |= other.input_unknown;
        self.output_unknown |= other.output_unknown;
        self.seconds += other.seconds;
        self.rounds = self.rounds.saturating_add(other.rounds);
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        if self.rounds == 0
            || self.output_unknown
            || !self.seconds.is_finite()
            || self.seconds <= 0.0
        {
            return None;
        }
        let rate = self.output_tokens as f64 / self.seconds;
        rate.is_finite().then_some(rate)
    }
}

pub fn client() -> genai::Result<Client> {
    let web = WebConfig::default().with_connect_timeout(CONNECT_TIMEOUT);
    let reqwest = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    let reqwest =
        web.apply_to_builder(reqwest)
            .build()
            .map_err(|error| genai::Error::ClientBuildFail {
                cause: error.to_string(),
            })?;
    Client::builder().with_reqwest(reqwest).build()
}

pub fn definitions() -> Vec<Tool> {
    vec![
        Tool::new("read_file").with_description("Read UTF-8 text after user permission. Sensitive files are prohibited.")
            .with_schema(json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})),
        Tool::new("list_directory").with_description("List a directory after user permission.")
            .with_schema(json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})),
        Tool::new("webfetch").with_description("Fetch a public HTTP(S) URL after user permission to browse. Returns UTF-8 text or HTML (no JavaScript). No credentials, redirects, or private-network access.")
            .with_schema(json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"],"additionalProperties":false})),
        Tool::new("propose_edit").with_description("Propose a complete replacement for the open document snapshot. Never saves or applies changes; the user reviews the diff explicitly.")
            .with_schema(json!({"type":"object","properties":{"replacement":{"type":"string"}},"required":["replacement"],"additionalProperties":false})),
    ]
}

/// Only library error categories are exposed: provider bodies can echo credentials/prompts.
pub fn safe_error(error: &genai::Error) -> String {
    match error {
        genai::Error::HttpError { status, .. } => format!("Provider returned HTTP {status}; check credentials, model access, and rate limits"),
        genai::Error::RequiresApiKey { .. } | genai::Error::NoAuthData { .. } | genai::Error::Resolver { .. } => "Provider authentication/resolution failed; check its configured environment, keyring, or private-file credential".into(),
        _ => "Provider request/stream failed; check model, endpoint, credentials, and network. Sensitive provider diagnostics are suppressed.".into(),
    }
}

pub async fn run(request: Request, sender: mpsc::Sender<Event>) {
    let result = tokio::time::timeout(REQUEST_TIMEOUT, conversation(request, &sender)).await;
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e.to_string(),
        Err(_) => format!(
            "Agent request timed out after {} minutes (including permission waits)",
            REQUEST_TIMEOUT.as_secs() / 60
        ),
    };
    let _ = sender.send(Event::Failed(error)).await;
}

async fn conversation(request: Request, sender: &mpsc::Sender<Event>) -> Result<()> {
    let client = client().map_err(|e| anyhow::anyhow!(safe_error(&e)))?;
    let target = if let Some(provider) = &request.provider {
        let model = request
            .model
            .split_once("::")
            .map(|(_, model)| model)
            .context("Custom provider model is missing its namespace")?;
        crate::provider::target(provider, model)?
    } else {
        client
            .resolve_service_target(request.model.as_str())
            .await
            .map_err(|e| anyhow::anyhow!(safe_error(&e)))?
    };
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
        .with_capture_usage(true)
        .with_capture_tool_calls(true)
        .with_capture_reasoning_content(true)
        .with_stream_header_timeout(STREAM_TIMEOUT)
        .with_stream_read_timeout(STREAM_TIMEOUT);
    let mut total = 0;
    let mut budget = TurnBudget::default();
    let mut proposed = false;
    loop {
        budget.next_generation()?;
        sender.send(Event::RequestStarted).await?;
        let started = Instant::now();
        let mut stream = client
            .exec_chat_stream(target.clone(), chat.clone(), Some(&options))
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
            anyhow::anyhow!("Provider stream ended without a completion marker; no edits applied")
        })?;
        sender
            .send(Event::Usage(RequestUsage::from_provider(
                end.captured_usage.as_ref(),
                started.elapsed(),
            )))
            .await?;
        if end
            .captured_stop_reason
            .as_ref()
            .is_some_and(|r| r.is_max_tokens())
        {
            bail!("Provider response was truncated; no edits applied");
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
            budget.record_tool_call()?;
            let result = if call.fn_name == "propose_edit" {
                if !request.allow_edits {
                    Err(anyhow::anyhow!(
                        "Document editing is unavailable in Chat mode"
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
                    Ok("Edit queued. Queditor will validate, apply and save it after successful completion, then show Keep/Undo review. It is not saved yet.".into())
                }
            } else if matches!(
                call.fn_name.as_str(),
                "read_file" | "list_directory" | "webfetch"
            ) {
                let web = call.fn_name == "webfetch";
                let path =
                    string_arg(&call.fn_arguments, if web { "url" } else { "path" })?.to_owned();
                let workspace = request.workspace.clone();
                let op = call.fn_name.clone();
                match tokio::task::spawn_blocking(move || {
                    if web {
                        tools::prepare_web(&workspace, &path)
                    } else {
                        tools::prepare(&workspace, &path, &op)
                    }
                })
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
                            tools::execute_approved(target).await
                        } else {
                            Err(anyhow::anyhow!(if web {
                                "User denied permission to browse"
                            } else {
                                "User denied file access"
                            }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_budget_allows_long_tool_sequences_and_a_final_response() {
        let mut budget = TurnBudget::default();
        for _ in 0..99 {
            budget.next_generation().unwrap();
            budget.record_tool_call().unwrap();
        }
        // A tool-free final response is still allowed on the hundredth generation.
        budget.next_generation().unwrap();
        assert_eq!(budget.generations, 100);
        assert_eq!(budget.tool_calls, 99);
        let error = budget.next_generation().unwrap_err().to_string();
        assert!(error.contains("100-generation limit"));
        assert!(error.contains("no edits applied"));
        assert_eq!(budget.generations, 100);
    }

    #[test]
    fn turn_budget_bounds_multiple_tool_calls_per_generation() {
        let mut budget = TurnBudget::default();
        budget.next_generation().unwrap();
        for _ in 0..100 {
            budget.record_tool_call().unwrap();
        }
        let error = budget.record_tool_call().unwrap_err().to_string();
        assert!(error.contains("100-tool-call limit"));
        assert!(error.contains("no edits applied"));
        assert_eq!(budget.tool_calls, 100);
        // Exhausting tool calls does not prevent a tool-free final response.
        budget.next_generation().unwrap();
    }

    #[test]
    fn live_timeouts_allow_slow_providers_and_permission_waits() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(60));
        assert_eq!(STREAM_TIMEOUT, Duration::from_secs(1800));
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(1800));
        assert!(CONNECT_TIMEOUT < STREAM_TIMEOUT);
        assert!(STREAM_TIMEOUT <= REQUEST_TIMEOUT);
    }

    #[test]
    fn usage_sums_tool_rounds_and_uses_output_tokens_over_provider_time() {
        let usage = Usage {
            prompt_tokens: Some(100),
            completion_tokens: Some(20),
            ..Default::default()
        };
        let mut total = RequestUsage::from_provider(Some(&usage), Duration::from_secs(2));
        total.add(&RequestUsage::from_provider(
            Some(&usage),
            Duration::from_secs(3),
        ));
        assert_eq!(
            (total.input_tokens, total.output_tokens, total.rounds),
            (200, 40, 2)
        );
        assert_eq!(total.tokens_per_second(), Some(8.0));
        let restored: RequestUsage =
            serde_json::from_str(&serde_json::to_string(&total).unwrap()).unwrap();
        assert_eq!(restored, total);
    }

    #[test]
    fn missing_negative_and_zero_duration_usage_never_fabricate_speed() {
        let usage = Usage {
            prompt_tokens: Some(-1),
            completion_tokens: Some(0),
            ..Default::default()
        };
        let mut total = RequestUsage::from_provider(Some(&usage), Duration::ZERO);
        assert!(total.input_unknown);
        assert!(!total.output_unknown);
        assert_eq!(total.tokens_per_second(), None);
        total.add(&RequestUsage::from_provider(None, Duration::from_secs(2)));
        assert!(total.output_unknown);
        assert_eq!(total.tokens_per_second(), None);
        assert_eq!(RequestUsage::default().tokens_per_second(), None);
    }
}
