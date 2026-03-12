use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;
use tracing::warn;

use std::collections::HashSet;
use uuid::Uuid;

use crate::claude::{
    ContentBlock, ImageSource, Message, MessageContent, MessagesRequest, MessagesResponse,
    ResponseContentBlock, ToolDefinition, Usage,
};
use crate::config::Config;
use crate::error::FinallyAValueBotError;

/// Remove orphaned `ToolResult` blocks whose `tool_use_id` does not match any
/// `ToolUse` block in the conversation.  This can happen after session
/// compaction splits a tool_use / tool_result pair.
fn sanitize_messages(messages: Vec<Message>) -> Vec<Message> {
    // Collect all tool_use IDs from assistant messages (owned to avoid borrow conflicts).
    let known_ids: HashSet<String> = messages
        .iter()
        .filter(|m| m.role == "assistant")
        .flat_map(|m| match &m.content {
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect();

    messages
        .into_iter()
        .filter_map(|msg| {
            if msg.role != "user" {
                return Some(msg);
            }
            match msg.content {
                MessageContent::Blocks(blocks) => {
                    let filtered: Vec<ContentBlock> = blocks
                        .into_iter()
                        .filter(|b| match b {
                            ContentBlock::ToolResult { tool_use_id, .. } => {
                                known_ids.contains(tool_use_id)
                            }
                            _ => true,
                        })
                        .collect();
                    if filtered.is_empty() {
                        None // Drop entirely empty user messages
                    } else {
                        Some(Message {
                            role: msg.role,
                            content: MessageContent::Blocks(filtered),
                        })
                    }
                }
                other => Some(Message {
                    role: msg.role,
                    content: other,
                }),
            }
        })
        .collect()
}

#[derive(Default)]
struct SseEventParser {
    pending: String,
    data_lines: Vec<String>,
}

impl SseEventParser {
    fn push_chunk(&mut self, chunk: &str) -> Vec<String> {
        self.pending.push_str(chunk);
        let mut events = Vec::new();

        while let Some(pos) = self.pending.find('\n') {
            let mut line = self.pending[..pos].to_string();
            self.pending = self.pending[pos + 1..].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            if let Some(event_data) = self.handle_line(&line) {
                events.push(event_data);
            }
        }

        events
    }

    fn finish(&mut self) -> Vec<String> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            let mut line = std::mem::take(&mut self.pending);
            if line.ends_with('\r') {
                line.pop();
            }
            if let Some(event_data) = self.handle_line(&line) {
                events.push(event_data);
            }
        }
        if let Some(event_data) = self.flush_event() {
            events.push(event_data);
        }
        events
    }

    fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.is_empty() {
            return self.flush_event();
        }
        if line.starts_with(':') {
            return None;
        }

        let (field, value) = match line.split_once(':') {
            Some((f, v)) => {
                let v = v.strip_prefix(' ').unwrap_or(v);
                (f, v)
            }
            None => (line, ""),
        };

        if field == "data" {
            self.data_lines.push(value.to_string());
        }
        None
    }

    fn flush_event(&mut self) -> Option<String> {
        if self.data_lines.is_empty() {
            return None;
        }
        let data = self.data_lines.join("\n");
        self.data_lines.clear();
        Some(data)
    }
}

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn send_message(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError>;

    async fn send_message_stream(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        text_tx: Option<&UnboundedSender<String>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError> {
        let response = self.send_message(system, messages, tools).await?;
        if let Some(tx) = text_tx {
            for block in &response.content {
                if let ResponseContentBlock::Text { text } = block {
                    let _ = tx.send(text.clone());
                }
            }
        }
        Ok(response)
    }
}

pub fn create_provider(config: &Config) -> Box<dyn LlmProvider> {
    match config.llm_provider.trim().to_lowercase().as_str() {
        "anthropic" => Box::new(AnthropicProvider::new(config)),
        "google" | "gemini" => Box::new(GeminiProvider::new(config)),
        _ => Box::new(OpenAiProvider::new(config)),
    }
}

/// Test that a model override is reachable with the current provider/config.
/// Returns Ok(()) on success, or an error string suitable for showing to the user.
pub async fn test_model(config: &Config, model_override: &str) -> Result<(), String> {
    let mut test_config = config.clone();
    test_config.model = model_override.to_string();
    let provider = create_provider(&test_config);
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text("Hi".into()),
    }];
    match provider
        .send_message("Test.", messages, None)
        .await
    {
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Anthropic provider
// ---------------------------------------------------------------------------

pub struct AnthropicProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(config: &Config) -> Self {
        AnthropicProvider {
            http: reqwest::Client::new(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            base_url: config
                .llm_base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1/messages".into()),
        }
    }

    async fn send_message_stream_single_pass(
        &self,
        request: &MessagesRequest,
        text_tx: Option<&UnboundedSender<String>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError> {
        let mut streamed_request = request.clone();
        streamed_request.stream = Some(true);

        let response = self
            .http
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&streamed_request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            if let Ok(api_err) = serde_json::from_str::<AnthropicApiError>(&body) {
                return Err(FinallyAValueBotError::LlmApi(format!(
                    "{}: {}",
                    api_err.error.error_type, api_err.error.message
                )));
            }
            return Err(FinallyAValueBotError::LlmApi(format!("HTTP {status}: {body}")));
        }

        let mut byte_stream = response.bytes_stream();
        let mut sse = SseEventParser::default();
        let mut stop_reason: Option<String> = None;
        let mut usage: Option<Usage> = None;
        let mut text_blocks: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();
        let mut tool_blocks: std::collections::HashMap<usize, StreamToolUseBlock> =
            std::collections::HashMap::new();
        let mut ordered_indexes: Vec<usize> = Vec::new();

        'outer: while let Some(chunk_res) = byte_stream.next().await {
            let chunk = match chunk_res {
                Ok(c) => c,
                Err(_) => break,
            };
            for data in sse.push_chunk(&String::from_utf8_lossy(&chunk)) {
                if data == "[DONE]" {
                    break 'outer;
                }
                process_anthropic_stream_event(
                    &data,
                    text_tx,
                    &mut stop_reason,
                    &mut usage,
                    &mut text_blocks,
                    &mut tool_blocks,
                    &mut ordered_indexes,
                );
            }
        }
        for data in sse.finish() {
            if data == "[DONE]" {
                break;
            }
            process_anthropic_stream_event(
                &data,
                text_tx,
                &mut stop_reason,
                &mut usage,
                &mut text_blocks,
                &mut tool_blocks,
                &mut ordered_indexes,
            );
        }

        Ok(build_stream_response(
            ordered_indexes,
            text_blocks,
            tool_blocks,
            stop_reason,
            usage,
        ))
    }
}

#[derive(Default)]
struct StreamToolUseBlock {
    id: String,
    name: String,
    input_json: String,
    thought_signature: Option<String>,
}

fn usage_from_json(v: &serde_json::Value) -> Option<Usage> {
    let input = v.get("input_tokens").and_then(|n| n.as_u64())?;
    let output = v
        .get("output_tokens")
        .and_then(|n| n.as_u64())
        .or_else(|| v.get("completion_tokens").and_then(|n| n.as_u64()))
        .unwrap_or(0);
    Some(Usage {
        input_tokens: u32::try_from(input).unwrap_or(u32::MAX),
        output_tokens: u32::try_from(output).unwrap_or(u32::MAX),
    })
}

fn process_anthropic_stream_event(
    data: &str,
    text_tx: Option<&UnboundedSender<String>>,
    stop_reason: &mut Option<String>,
    usage: &mut Option<Usage>,
    text_blocks: &mut std::collections::HashMap<usize, String>,
    tool_blocks: &mut std::collections::HashMap<usize, StreamToolUseBlock>,
    ordered_indexes: &mut Vec<usize>,
) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
        return;
    };

    let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or_default();
    match event_type {
        "content_block_start" => {
            if let Some(index) = v
                .get("index")
                .and_then(|i| i.as_u64())
                .and_then(|i| usize::try_from(i).ok())
            {
                if !ordered_indexes.contains(&index) {
                    ordered_indexes.push(index);
                }
                if let Some(block) = v.get("content_block") {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            let text = block
                                .get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or_default()
                                .to_string();
                            text_blocks.insert(index, text);
                        }
                        Some("tool_use") => {
                            let id = block
                                .get("id")
                                .and_then(|s| s.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let name = block
                                .get("name")
                                .and_then(|s| s.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                            let input_json = if input.is_object()
                                && input.as_object().is_some_and(|m| m.is_empty())
                            {
                                String::new()
                            } else {
                                serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
                            };
                            tool_blocks.insert(
                                index,
                                StreamToolUseBlock {
                                    id,
                                    name,
                                    input_json,
                                    thought_signature: block
                                        .get("thought_signature")
                                        .or_else(|| block.get("thoughtSignature"))
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                },
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
        "content_block_delta" => {
            let Some(index) = v
                .get("index")
                .and_then(|i| i.as_u64())
                .and_then(|i| usize::try_from(i).ok())
            else {
                return;
            };
            let Some(delta) = v.get("delta") else {
                return;
            };
            match delta.get("type").and_then(|t| t.as_str()) {
                Some("text_delta") => {
                    let piece = delta
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default();
                    if !piece.is_empty() {
                        text_blocks.entry(index).or_default().push_str(piece);
                        if let Some(tx) = text_tx {
                            let _ = tx.send(piece.to_string());
                        }
                    }
                }
                Some("input_json_delta") => {
                    let piece = delta
                        .get("partial_json")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default();
                    if !piece.is_empty() {
                        tool_blocks
                            .entry(index)
                            .or_default()
                            .input_json
                            .push_str(piece);
                    }
                }
                _ => {}
            }
        }
        "message_delta" => {
            if let Some(reason) = v
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|s| s.as_str())
            {
                *stop_reason = Some(reason.to_string());
            }
            if let Some(u) = v.get("usage") {
                *usage = usage_from_json(u);
            }
        }
        "message_start" => {
            if let Some(u) = v.get("message").and_then(|m| m.get("usage")) {
                *usage = usage_from_json(u);
            }
        }
        _ => {}
    }
}

fn process_openai_stream_event(
    data: &str,
    text_tx: Option<&UnboundedSender<String>>,
    text: &mut String,
    stop_reason: &mut Option<String>,
    usage: &mut Option<Usage>,
    tool_calls: &mut std::collections::BTreeMap<usize, StreamToolUseBlock>,
) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
        return;
    };

    if usage.is_none() {
        *usage = v.get("usage").and_then(usage_from_json);
    }

    let Some(choice) = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
    else {
        return;
    };

    if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
        *stop_reason = Some(reason.to_string());
    }

    let Some(delta) = choice.get("delta") else {
        return;
    };

    if let Some(piece) = delta.get("content").and_then(|t| t.as_str()) {
        if !piece.is_empty() {
            text.push_str(piece);
            if let Some(tx) = text_tx {
                let _ = tx.send(piece.to_string());
            }
        }
    }

    if let Some(tc_arr) = delta.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in tc_arr {
            let Some(index) = tc
                .get("index")
                .and_then(|i| i.as_u64())
                .and_then(|i| usize::try_from(i).ok())
            else {
                continue;
            };
            let entry = tool_calls.entry(index).or_default();
            if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                entry.id = id.to_string();
            }
            if let Some(s) = tc.get("thought_signature").or_else(|| tc.get("thoughtSignature")).and_then(|v| v.as_str()) {
                entry.thought_signature = Some(s.to_string());
            }
            if let Some(function) = tc.get("function") {
                if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                    entry.name = name.to_string();
                }
                if let Some(args) = function.get("arguments").and_then(|v| v.as_str()) {
                    entry.input_json.push_str(args);
                }
                if entry.thought_signature.is_none() {
                    if let Some(s) = function.get("thought_signature").or_else(|| function.get("thoughtSignature")).and_then(|v| v.as_str()) {
                        entry.thought_signature = Some(s.to_string());
                    }
                }
            }
        }
    }
}

fn normalize_stop_reason(reason: Option<String>) -> Option<String> {
    match reason.as_deref() {
        Some("tool_use") | Some("tool_calls") => Some("tool_use".into()),
        Some("max_tokens") | Some("length") => Some("max_tokens".into()),
        Some("stop") | Some("end_turn") | None => Some("end_turn".into()),
        Some(other) => Some(other.to_string()),
    }
}

fn parse_tool_input(input_json: &str) -> serde_json::Value {
    let trimmed = input_json.trim();
    if trimmed.is_empty() {
        return json!({});
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| json!({}))
}

fn build_stream_response(
    ordered_indexes: Vec<usize>,
    text_blocks: std::collections::HashMap<usize, String>,
    tool_blocks: std::collections::HashMap<usize, StreamToolUseBlock>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
) -> MessagesResponse {
    let mut content = Vec::new();
    for index in ordered_indexes {
        if let Some(text) = text_blocks.get(&index) {
            if !text.is_empty() {
                content.push(ResponseContentBlock::Text { text: text.clone() });
            }
        }
        if let Some(tool) = tool_blocks.get(&index) {
            content.push(ResponseContentBlock::ToolUse {
                id: tool.id.clone(),
                name: tool.name.clone(),
                input: parse_tool_input(&tool.input_json),
                thought_signature: tool.thought_signature.clone(),
            });
        }
    }

    if content.is_empty() {
        content.push(ResponseContentBlock::Text {
            text: String::new(),
        });
    }

    MessagesResponse {
        content,
        stop_reason: normalize_stop_reason(stop_reason),
        usage,
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicApiError {
    error: AnthropicApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct AnthropicApiErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn send_message(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError> {
        let messages = sanitize_messages(messages);

        let request = MessagesRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: system.to_string(),
            messages,
            tools,
            stream: None,
        };

        let mut retries = 0u32;
        let max_retries = 3;

        loop {
            let response = self
                .http
                .post(&self.base_url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&request)
                .send()
                .await?;

            let status = response.status();

            if status.is_success() {
                let body = response.text().await?;
                let parsed: MessagesResponse = serde_json::from_str(&body).map_err(|e| {
                    FinallyAValueBotError::LlmApi(format!("Failed to parse response: {e}\nBody: {body}"))
                })?;
                return Ok(parsed);
            }

            if status.as_u16() == 429 && retries < max_retries {
                retries += 1;
                let delay = std::time::Duration::from_secs(2u64.pow(retries));
                warn!(
                    "Rate limited, retrying in {:?} (attempt {retries}/{max_retries})",
                    delay
                );
                tokio::time::sleep(delay).await;
                continue;
            }

            let body = response.text().await.unwrap_or_default();
            if let Ok(api_err) = serde_json::from_str::<AnthropicApiError>(&body) {
                return Err(FinallyAValueBotError::LlmApi(format!(
                    "{}: {}",
                    api_err.error.error_type, api_err.error.message
                )));
            }
            return Err(FinallyAValueBotError::LlmApi(format!("HTTP {status}: {body}")));
        }
    }

    async fn send_message_stream(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        text_tx: Option<&UnboundedSender<String>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError> {
        let messages = sanitize_messages(messages);
        let request = MessagesRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: system.to_string(),
            messages,
            tools,
            stream: Some(true),
        };

        self.send_message_stream_single_pass(&request, text_tx)
            .await
    }
}

// ---------------------------------------------------------------------------
// OpenAI-compatible provider  (OpenAI, OpenRouter, DeepSeek, Groq, Ollama …)
// ---------------------------------------------------------------------------

pub struct OpenAiProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    chat_url: String,
}

impl OpenAiProvider {
    pub fn new(config: &Config) -> Self {
        let base = config
            .llm_base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        let chat_url = format!("{}/chat/completions", base.trim_end_matches('/'));

        OpenAiProvider {
            http: reqwest::Client::new(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            chat_url,
        }
    }
}

// --- OpenAI response types ---

#[derive(Debug, Deserialize)]
struct OaiResponse {
    choices: Vec<OaiChoice>,
    usage: Option<OaiUsage>,
}

#[derive(Debug, Deserialize)]
struct OaiChoice {
    message: OaiMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OaiMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OaiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OaiToolCall {
    id: String,
    function: OaiFunction,
    #[serde(default, alias = "thoughtSignature")]
    thought_signature: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OaiFunction {
    name: String,
    arguments: String,
    #[serde(default, alias = "thoughtSignature")]
    thought_signature: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OaiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct OaiErrorResponse {
    error: OaiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct OaiErrorDetail {
    message: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
}

fn format_oai_error(status: reqwest::StatusCode, err: &OaiErrorDetail, body: &str) -> String {
    let status = status.as_u16();
    let extra: Vec<String> = [
        err.r#type.as_deref().map(|t| format!("type={t}")),
        err.code.as_deref().map(|c| format!("code={c}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let extra_str = if extra.is_empty() {
        String::new()
    } else {
        format!(" ({})", extra.join(", "))
    };
    let body_preview = if body.len() > 400 {
        format!("{}...", &body[..400])
    } else {
        body.to_string()
    };
    format!(
        "HTTP {}: {}{}. Response: {}",
        status, err.message, extra_str, body_preview
    )
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn send_message(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError> {
        let oai_messages = translate_messages_to_oai(system, &messages);

        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": oai_messages,
        });

        if let Some(ref tool_defs) = tools {
            if !tool_defs.is_empty() {
                body["tools"] = json!(translate_tools_to_oai(tool_defs));
            }
        }

        let mut retries = 0u32;
        let max_retries = 3;

        loop {
            let mut req = self
                .http
                .post(&self.chat_url)
                .header("Content-Type", "application/json")
                .json(&body);
            if !self.api_key.trim().is_empty() {
                req = req.header("Authorization", format!("Bearer {}", self.api_key));
            }
            let response = req.send().await?;

            let status = response.status();

            if status.is_success() {
                let text = response.text().await?;
                let oai: OaiResponse = serde_json::from_str(&text).map_err(|e| {
                    FinallyAValueBotError::LlmApi(format!(
                        "Failed to parse OpenAI response: {e}\nBody: {text}"
                    ))
                })?;
                return Ok(translate_oai_response(oai));
            }

            if status.as_u16() == 429 && retries < max_retries {
                retries += 1;
                let delay = std::time::Duration::from_secs(2u64.pow(retries));
                warn!(
                    "Rate limited, retrying in {:?} (attempt {retries}/{max_retries})",
                    delay
                );
                tokio::time::sleep(delay).await;
                continue;
            }

            let text = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<OaiErrorResponse>(&text) {
                let msg = format_oai_error(status, &err.error, &text);
                return Err(FinallyAValueBotError::LlmApi(msg));
            }
            return Err(FinallyAValueBotError::LlmApi(format!("HTTP {status}: {text}")));
        }
    }

    async fn send_message_stream(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        text_tx: Option<&UnboundedSender<String>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError> {
        let oai_messages = translate_messages_to_oai(system, &messages);

        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": oai_messages,
            "stream": true,
        });

        if let Some(ref tool_defs) = tools {
            if !tool_defs.is_empty() {
                body["tools"] = json!(translate_tools_to_oai(tool_defs));
            }
        }

        let mut req = self
            .http
            .post(&self.chat_url)
            .header("Content-Type", "application/json")
            .json(&body);
        if !self.api_key.trim().is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }
        let response = req.send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<OaiErrorResponse>(&text) {
                let msg = format_oai_error(status, &err.error, &text);
                return Err(FinallyAValueBotError::LlmApi(msg));
            }
            return Err(FinallyAValueBotError::LlmApi(format!("HTTP {status}: {text}")));
        }

        let mut byte_stream = response.bytes_stream();
        let mut sse = SseEventParser::default();
        let mut text = String::new();
        let mut stop_reason: Option<String> = None;
        let mut usage: Option<Usage> = None;
        let mut tool_calls: std::collections::BTreeMap<usize, StreamToolUseBlock> =
            std::collections::BTreeMap::new();

        'outer: while let Some(chunk_res) = byte_stream.next().await {
            let chunk = match chunk_res {
                Ok(c) => c,
                Err(_) => break,
            };
            for data in sse.push_chunk(&String::from_utf8_lossy(&chunk)) {
                if data == "[DONE]" {
                    break 'outer;
                }
                process_openai_stream_event(
                    &data,
                    text_tx,
                    &mut text,
                    &mut stop_reason,
                    &mut usage,
                    &mut tool_calls,
                );
            }
        }
        for data in sse.finish() {
            if data == "[DONE]" {
                break;
            }
            process_openai_stream_event(
                &data,
                text_tx,
                &mut text,
                &mut stop_reason,
                &mut usage,
                &mut tool_calls,
            );
        }

        let mut content = Vec::new();
        if !text.is_empty() {
            content.push(ResponseContentBlock::Text { text });
        }
        for (_index, tool) in tool_calls {
            content.push(ResponseContentBlock::ToolUse {
                id: tool.id,
                name: tool.name,
                input: parse_tool_input(&tool.input_json),
                thought_signature: tool.thought_signature,
            });
        }
        if content.is_empty() {
            content.push(ResponseContentBlock::Text {
                text: String::new(),
            });
        }

        Ok(MessagesResponse {
            content,
            stop_reason: normalize_stop_reason(stop_reason),
            usage,
        })
    }
}

// ---------------------------------------------------------------------------
// Google Gemini native provider
// ---------------------------------------------------------------------------

pub struct GeminiProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl GeminiProvider {
    pub fn new(config: &Config) -> Self {
        GeminiProvider {
            http: reqwest::Client::new(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
        }
    }

    fn generate_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        )
    }

    fn stream_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.model, self.api_key
        )
    }
}

#[derive(Debug, Deserialize)]
struct GeminiError {
    error: GeminiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorDetail {
    code: i32,
    message: String,
    #[allow(dead_code)]
    #[serde(default)]
    status: Option<String>,
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn send_message(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError> {
        let messages = sanitize_messages(messages);
        let request_body = build_gemini_request(system, &messages, tools, self.max_tokens);

        let mut retries = 0u32;
        let max_retries = 3;

        loop {
            let response = self
                .http
                .post(&self.generate_url())
                .header("Content-Type", "application/json")
                .json(&request_body)
                .send()
                .await?;

            let status = response.status();

            if status.is_success() {
                let body = response.text().await?;
                return parse_gemini_response(&body);
            }

            if status.as_u16() == 429 && retries < max_retries {
                retries += 1;
                let delay = std::time::Duration::from_secs(2u64.pow(retries));
                warn!(
                    "Rate limited, retrying in {:?} (attempt {retries}/{max_retries})",
                    delay
                );
                tokio::time::sleep(delay).await;
                continue;
            }

            let body = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<GeminiError>(&body) {
                return Err(FinallyAValueBotError::LlmApi(format!(
                    "Gemini API error {}: {}",
                    err.error.code, err.error.message
                )));
            }
            return Err(FinallyAValueBotError::LlmApi(format!("HTTP {status}: {body}")));
        }
    }

    async fn send_message_stream(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        text_tx: Option<&UnboundedSender<String>>,
    ) -> Result<MessagesResponse, FinallyAValueBotError> {
        let messages = sanitize_messages(messages);
        let request_body = build_gemini_request(system, &messages, tools, self.max_tokens);

        let response = self
            .http
            .post(&self.stream_url())
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<GeminiError>(&text) {
                return Err(FinallyAValueBotError::LlmApi(format!(
                    "Gemini API error {}: {}",
                    err.error.code, err.error.message
                )));
            }
            return Err(FinallyAValueBotError::LlmApi(format!("HTTP {status}: {text}")));
        }

        let mut byte_stream = response.bytes_stream();
        let mut sse = SseEventParser::default();
        let mut accumulated_text = String::new();
        let mut tool_calls: Vec<(String, String, serde_json::Value, Option<String>)> = Vec::new();
        let mut stop_reason: Option<String> = None;
        let mut usage: Option<Usage> = None;

        while let Some(chunk_res) = byte_stream.next().await {
            let chunk = match chunk_res {
                Ok(c) => c,
                Err(_) => break,
            };
            for data in sse.push_chunk(&String::from_utf8_lossy(&chunk)) {
                process_gemini_stream_event(
                    &data,
                    text_tx,
                    &mut accumulated_text,
                    &mut tool_calls,
                    &mut stop_reason,
                    &mut usage,
                );
            }
        }
        for data in sse.finish() {
            process_gemini_stream_event(
                &data,
                text_tx,
                &mut accumulated_text,
                &mut tool_calls,
                &mut stop_reason,
                &mut usage,
            );
        }

        let has_tool_calls = !tool_calls.is_empty();
        let mut content = Vec::new();
        if !accumulated_text.is_empty() {
            content.push(ResponseContentBlock::Text {
                text: accumulated_text,
            });
        }
        for (id, name, input, thought_sig) in tool_calls {
            content.push(ResponseContentBlock::ToolUse {
                id,
                name,
                input,
                thought_signature: thought_sig,
            });
        }
        if content.is_empty() {
            content.push(ResponseContentBlock::Text {
                text: String::new(),
            });
        }

        let final_stop_reason = if has_tool_calls {
            Some("tool_use".into())
        } else {
            stop_reason
        };

        Ok(MessagesResponse {
            content,
            stop_reason: normalize_stop_reason(final_stop_reason),
            usage,
        })
    }
}

fn process_gemini_stream_event(
    data: &str,
    text_tx: Option<&UnboundedSender<String>>,
    accumulated_text: &mut String,
    tool_calls: &mut Vec<(String, String, serde_json::Value, Option<String>)>,
    stop_reason: &mut Option<String>,
    usage: &mut Option<Usage>,
) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
        return;
    };

    if let Some(candidates) = v.get("candidates").and_then(|c| c.as_array()) {
        if let Some(candidate) = candidates.first() {
            if let Some(reason) = candidate
                .get("finishReason")
                .and_then(|r| r.as_str())
            {
                *stop_reason = Some(reason.to_string());
            }

            if let Some(content) = candidate.get("content") {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        // Text parts (skip if thought == true)
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            let is_thought = part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false);
                            if !is_thought {
                                accumulated_text.push_str(text);
                                if let Some(tx) = text_tx {
                                    let _ = tx.send(text.to_string());
                                }
                            }
                        }
                        // Function call parts
                        if let Some(fc) = part.get("functionCall") {
                            if let (Some(name), Some(args)) =
                                (fc.get("name").and_then(|n| n.as_str()),
                                 fc.get("args"))
                            {
                                let id = Uuid::new_v4().to_string();
                                let thought_sig = part
                                    .get("thoughtSignature")
                                    .or_else(|| fc.get("thoughtSignature"))
                                    .and_then(|ts| ts.as_str())
                                    .map(String::from);
                                tool_calls.push((id, name.to_string(), args.clone(), thought_sig));
                            }
                        }
                    }
                }
            }
        }
    }

    // Capture usage from any chunk
    if usage.is_none() {
        if let Some(metadata) = v.get("usageMetadata") {
            *usage = Some(Usage {
                input_tokens: metadata
                    .get("promptTokenCount")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0) as u32,
                output_tokens: metadata
                    .get("candidatesTokenCount")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0) as u32,
            });
        }
    }
}

fn build_gemini_request(
    system: &str,
    messages: &[Message],
    tools: Option<Vec<ToolDefinition>>,
    max_tokens: u32,
) -> serde_json::Value {
    let contents = translate_messages_to_gemini(messages);
    let mut request = json!({
        "contents": contents,
        "generationConfig": {
            "maxOutputTokens": max_tokens,
        }
    });

    if !system.is_empty() {
        request["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }

    if let Some(tool_defs) = tools {
        if !tool_defs.is_empty() {
            let func_decls: Vec<serde_json::Value> = tool_defs
                .iter()
                .map(|t| {
                    let parameters = sanitize_oai_parameters(&t.input_schema);
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": parameters,
                    })
                })
                .collect();
            request["tools"] = json!([{ "functionDeclarations": func_decls }]);
        }
    }

    request
}

fn translate_messages_to_gemini(messages: &[Message]) -> Vec<serde_json::Value> {
    // Build id → name map from all tool_use blocks
    let mut tool_id_to_name: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for msg in messages {
        if msg.role == "assistant" {
            if let MessageContent::Blocks(blocks) = &msg.content {
                for block in blocks {
                    if let ContentBlock::ToolUse { id, name, .. } = block {
                        tool_id_to_name.insert(id.clone(), name.clone());
                    }
                }
            }
        }
    }

    let mut out: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        let role = if msg.role == "assistant" { "model" } else { "user" };

        match &msg.content {
            MessageContent::Text(text) => {
                let content = if text.is_empty() {
                    GEMINI_MINIMAL_PARTS.to_string()
                } else {
                    text.clone()
                };
                out.push(json!({
                    "role": role,
                    "parts": [{ "text": content }]
                }));
            }
            MessageContent::Blocks(blocks) => {
                let mut parts: Vec<serde_json::Value> = Vec::new();

                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => {
                            parts.push(json!({ "text": text }));
                        }
                        ContentBlock::ToolUse {
                            name,
                            input,
                            thought_signature,
                            ..
                        } => {
                            let sig = thought_signature
                                .clone()
                                .unwrap_or_else(|| GEMINI_SKIP_THOUGHT_SIGNATURE.to_string());
                            let mut part = json!({
                                "functionCall": {
                                    "name": name,
                                    "args": input,
                                }
                            });
                            part["thoughtSignature"] = json!(sig);
                            parts.push(part);
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            // Look up function name
                            if let Some(fn_name) = tool_id_to_name.get(tool_use_id) {
                                let response = if is_error == &Some(true) {
                                    json!({"error": content})
                                } else {
                                    json!({"result": content})
                                };
                                parts.push(json!({
                                    "functionResponse": {
                                        "name": fn_name,
                                        "response": response,
                                    }
                                }));
                            }
                        }
                        ContentBlock::Image { source } => {
                            parts.push(json!({
                                "inlineData": {
                                    "mimeType": source.media_type,
                                    "data": source.data,
                                }
                            }));
                        }
                    }
                }

                if parts.is_empty() {
                    parts.push(json!({ "text": GEMINI_MINIMAL_PARTS }));
                }

                out.push(json!({
                    "role": role,
                    "parts": parts,
                }));
            }
        }
    }

    // Ensure first message is "user"
    if !out.is_empty() {
        let first_role = out[0].get("role").and_then(|r| r.as_str());
        if first_role != Some("user") {
            out.insert(0, json!({"role": "user", "parts": [{"text": GEMINI_MINIMAL_PARTS}]}));
        }
    }

    out
}

fn parse_gemini_response(body: &str) -> Result<MessagesResponse, FinallyAValueBotError> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| FinallyAValueBotError::LlmApi(format!("Failed to parse Gemini response: {e}\nBody: {body}")))?;

    let mut content = Vec::new();
    let mut stop_reason: Option<String> = None;
    let mut usage: Option<Usage> = None;

    if let Some(candidates) = v.get("candidates").and_then(|c| c.as_array()) {
        if let Some(candidate) = candidates.first() {
            if let Some(reason) = candidate
                .get("finishReason")
                .and_then(|r| r.as_str())
            {
                stop_reason = Some(reason.to_string());
            }

            if let Some(parts) = candidate
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                let mut has_tool_calls = false;
                for part in parts {
                    // Text parts (skip if thought == true)
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        let is_thought =
                            part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false);
                        if !is_thought && !text.is_empty() {
                            content.push(ResponseContentBlock::Text {
                                text: text.to_string(),
                            });
                        }
                    }
                    // Function call parts
                    if let Some(fc) = part.get("functionCall") {
                        has_tool_calls = true;
                        if let (Some(name), Some(args)) =
                            (fc.get("name").and_then(|n| n.as_str()),
                             fc.get("args"))
                        {
                            let id = Uuid::new_v4().to_string();
                            let thought_sig = part
                                .get("thoughtSignature")
                                .or_else(|| fc.get("thoughtSignature"))
                                .and_then(|ts| ts.as_str())
                                .map(String::from);
                            content.push(ResponseContentBlock::ToolUse {
                                id,
                                name: name.to_string(),
                                input: args.clone(),
                                thought_signature: thought_sig,
                            });
                        }
                    }
                }

                // Override stop_reason if tool calls present
                if has_tool_calls {
                    stop_reason = Some("tool_use".into());
                }
            }
        }
    }

    // Capture usage
    if let Some(metadata) = v.get("usageMetadata") {
        usage = Some(Usage {
            input_tokens: metadata
                .get("promptTokenCount")
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: metadata
                .get("candidatesTokenCount")
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
        });
    }

    if content.is_empty() {
        content.push(ResponseContentBlock::Text {
            text: String::new(),
        });
    }

    Ok(MessagesResponse {
        content,
        stop_reason: normalize_stop_reason(stop_reason),
        usage,
    })
}

// ---------------------------------------------------------------------------
// Format translation helpers  (internal Anthropic-style ↔ OpenAI)
// ---------------------------------------------------------------------------

/// Minimal non-whitespace content so Vertex AI / Gemini gateways never treat the prompt as "no parts".
const GEMINI_MINIMAL_PARTS: &str = ".";

/// When Gemini returns a functionCall without thoughtSignature, or we cannot preserve it (e.g. after
/// session load), use this dummy value so Gemini skips signature validation. Using a random UUID
/// would cause "Corrupted thought signature" because Gemini expects the exact signature it returned.
const GEMINI_SKIP_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

fn translate_messages_to_oai(system: &str, messages: &[Message]) -> Vec<serde_json::Value> {
    // Collect all tool_use IDs present in assistant messages so we can
    // skip orphaned tool_results (e.g. after session compaction).
    let known_tool_ids: std::collections::HashSet<&str> = messages
        .iter()
        .filter(|m| m.role == "assistant")
        .flat_map(|m| match &m.content {
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect();

    let mut out: Vec<serde_json::Value> = Vec::new();

    // System message. Gemini requires at least one "parts" in the request; empty content can cause INVALID_ARGUMENT.
    if !system.is_empty() {
        out.push(json!({"role": "system", "content": system}));
    }

    for msg in messages {
        match &msg.content {
            MessageContent::Text(text) => {
                // Ensure non-empty content so Gemini/Vertex receives at least one parts field.
                let content = if text.is_empty() {
                    GEMINI_MINIMAL_PARTS
                } else {
                    text.as_str()
                };
                out.push(json!({"role": msg.role, "content": content}));
            }
            MessageContent::Blocks(blocks) => {
                if msg.role == "assistant" {
                    // Collect text and tool_calls
                    let text: String = blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    let tool_calls: Vec<serde_json::Value> = blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::ToolUse {
                                id,
                                name,
                                input,
                                thought_signature,
                            } => {
                                let args = serde_json::to_string(input).unwrap_or_default();
                                // Gemini requires thought_signature in function calls. Use preserved
                                // signature if available; otherwise use skip value to avoid "Corrupted
                                // thought signature" (a random UUID would fail validation).
                                let sig = thought_signature
                                    .clone()
                                    .unwrap_or_else(|| GEMINI_SKIP_THOUGHT_SIGNATURE.to_string());

                                let tool_call = json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": args,
                                        "thought_signature": &sig,
                                        "thoughtSignature": &sig,
                                    },
                                    "thought_signature": &sig,
                                    "thoughtSignature": &sig,
                                });
                                Some(tool_call)
                            }
                            _ => None,
                        })
                        .collect();

                    let mut m = json!({"role": "assistant"});
                    // Gemini/Vertex require every message to have at least one "parts"; assistant with only
                    // tool_calls has no content otherwise, so always set non-whitespace placeholder when empty.
                    let content = if text.is_empty() {
                        GEMINI_MINIMAL_PARTS
                    } else {
                        text.as_str()
                    };
                    m["content"] = json!(content);
                    if !tool_calls.is_empty() {
                        m["tool_calls"] = json!(tool_calls);
                    }
                    out.push(m);
                } else {
                    // User role — tool_results, images, or text
                    let has_tool_results = blocks
                        .iter()
                        .any(|b| matches!(b, ContentBlock::ToolResult { .. }));

                    if has_tool_results {
                        // Each tool result → separate "tool" message
                        // Skip orphaned tool_results whose IDs are not in any assistant message
                        for block in blocks {
                            if let ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                            } = block
                            {
                                if !known_tool_ids.contains(tool_use_id.as_str()) {
                                    continue;
                                }
                                let c = if is_error == &Some(true) {
                                    format!("[Error] {content}")
                                } else {
                                    content.clone()
                                };
                                out.push(json!({
                                    "role": "tool",
                                    "tool_call_id": tool_use_id,
                                    "content": c,
                                }));
                            }
                        }
                    } else {
                        // Images + text → multipart content array
                        let has_images = blocks
                            .iter()
                            .any(|b| matches!(b, ContentBlock::Image { .. }));
                        if has_images {
                            let parts: Vec<serde_json::Value> = blocks
                                .iter()
                                .filter_map(|b| match b {
                                    ContentBlock::Text { text } => {
                                        Some(json!({"type": "text", "text": text}))
                                    }
                                    ContentBlock::Image {
                                        source:
                                            ImageSource {
                                                media_type, data, ..
                                            },
                                    } => {
                                        let url = format!("data:{media_type};base64,{data}");
                                        Some(json!({
                                            "type": "image_url",
                                            "image_url": {"url": url}
                                        }))
                                    }
                                    _ => None,
                                })
                                .collect();
                            // Gemini/Vertex require at least one part; use non-whitespace placeholder if empty.
                            let content = if parts.is_empty() {
                                json!([{"type": "text", "text": GEMINI_MINIMAL_PARTS}])
                            } else {
                                json!(parts)
                            };
                            out.push(json!({"role": "user", "content": content}));
                        } else {
                            let text: String = blocks
                                .iter()
                                .filter_map(|b| match b {
                                    ContentBlock::Text { text } => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            let content = if text.is_empty() {
                                GEMINI_MINIMAL_PARTS.to_string()
                            } else {
                                text
                            };
                            out.push(json!({"role": "user", "content": content}));
                        }
                    }
                }
            }
        }
    }

    // Gemini/Vertex require the prompt to have at least one "parts" field. Only prepend a
    // placeholder user message when the first message is assistant or tool (so the prompt would
    // have no user content). When the first message is system, gateways typically put it in
    // system_instruction and use the next user message as the prompt — so we must not prepend
    // a fake "user: ." or the model will respond to "." and may return nothing for the real request.
    let first_role = out.first().and_then(|m| m.get("role").and_then(|r| r.as_str()));
    if !out.is_empty()
        && first_role != Some("user")
        && (first_role == Some("assistant") || first_role == Some("tool"))
    {
        out.insert(
            0,
            json!({"role": "user", "content": GEMINI_MINIMAL_PARTS}),
        );
    }

    out
}

/// Sanitize tool parameters schema so that `required` only lists keys present in
/// `properties`. Google Gemini rejects schemas where required[i] is not in properties.
/// MCP and other sources can send schemas with missing properties or required not in properties.
/// Strips "enum" from each property so gateways (e.g. OpenRouter→Gemini) don't drop the
/// property and leave required referencing missing keys.
fn sanitize_oai_parameters(schema: &serde_json::Value) -> serde_json::Value {
    let props_raw = schema
        .get("properties")
        .filter(|p| p.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    // Build properties with "enum" stripped per property; some gateways drop properties
    // that contain "enum" when converting to Gemini, causing required to reference missing keys.
    let props: serde_json::Map<String, serde_json::Value> = props_raw
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let cleaned = if let Some(obj) = v.as_object() {
                        let mut copy = serde_json::Map::new();
                        for (pk, pv) in obj {
                            if pk != "enum" {
                                copy.insert(pk.clone(), pv.clone());
                            }
                        }
                        serde_json::Value::Object(copy)
                    } else {
                        v.clone()
                    };
                    (k.clone(), cleaned)
                })
                .collect()
        })
        .unwrap_or_default();
    let props_keys: std::collections::HashSet<String> = props.keys().cloned().collect();
    let required_filtered: Vec<String> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter(|k| props_keys.contains(*k))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    // Deduplicate and sort required for deterministic output; omit key when empty (Gemini-safe).
    let required_set: std::collections::HashSet<String> = required_filtered.into_iter().collect();
    let mut required_vec: Vec<String> = required_set.into_iter().collect();
    required_vec.sort();

    let mut out = serde_json::Map::new();
    out.insert(
        "type".to_string(),
        schema.get("type").cloned().unwrap_or(json!("object")),
    );
    out.insert("properties".to_string(), serde_json::Value::Object(props));
    if !required_vec.is_empty() {
        out.insert("required".to_string(), json!(required_vec));
    }
    serde_json::Value::Object(out)
}

fn translate_tools_to_oai(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            let parameters = sanitize_oai_parameters(&t.input_schema);
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": parameters,
                }
            })
        })
        .collect()
}

fn translate_oai_response(oai: OaiResponse) -> MessagesResponse {
    let choice = match oai.choices.into_iter().next() {
        Some(c) => c,
        None => {
            return MessagesResponse {
                content: vec![ResponseContentBlock::Text {
                    text: "(empty response)".into(),
                }],
                stop_reason: Some("end_turn".into()),
                usage: None,
            };
        }
    };

    let mut content = Vec::new();

    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            content.push(ResponseContentBlock::Text { text });
        }
    }

    if let Some(tool_calls) = choice.message.tool_calls {
        for tc in tool_calls {
            let input: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or_default();
            let thought_signature = tc
                .thought_signature
                .clone()
                .or(tc.function.thought_signature.clone());
            content.push(ResponseContentBlock::ToolUse {
                id: tc.id,
                name: tc.function.name,
                input,
                thought_signature,
            });
        }
    }

    if content.is_empty() {
        content.push(ResponseContentBlock::Text {
            text: String::new(),
        });
    }

    let stop_reason = match choice.finish_reason.as_deref() {
        Some("tool_calls") => Some("tool_use".into()),
        Some("length") => Some("max_tokens".into()),
        _ => Some("end_turn".into()),
    };

    let usage = oai.usage.map(|u| Usage {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
    });

    MessagesResponse {
        content,
        stop_reason,
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // translate_messages_to_oai
    // -----------------------------------------------------------------------

    #[test]
    fn test_translate_messages_system_only() {
        let msgs: Vec<Message> = vec![];
        let out = translate_messages_to_oai("You are a bot.", &msgs);
        // We only prepend for assistant/tool; system-first is left as-is so gateways use first user as prompt.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out[0]["content"], "You are a bot.");
    }

    #[test]
    fn test_translate_messages_empty_system_omitted() {
        let msgs: Vec<Message> = vec![];
        let out = translate_messages_to_oai("", &msgs);
        assert!(out.is_empty());
    }

    #[test]
    fn test_translate_messages_text_roundtrip() {
        let msgs = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("hello".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("hi".into()),
            },
        ];
        let out = translate_messages_to_oai("sys", &msgs);
        // We do not prepend when first is system so the first user message is the prompt.
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out[1]["role"], "user");
        assert_eq!(out[1]["content"], "hello");
        assert_eq!(out[2]["role"], "assistant");
        assert_eq!(out[2]["content"], "hi");
    }

    #[test]
    fn test_translate_messages_assistant_tool_use() {
        let msgs = vec![Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "Let me check.".into(),
                },
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: json!({"command": "ls"}),
                    thought_signature: None,
                },
            ]),
        }];
        let out = translate_messages_to_oai("", &msgs);
        assert_eq!(out.len(), 2); // placeholder user first (Gemini), then assistant
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], GEMINI_MINIMAL_PARTS);
        assert_eq!(out[1]["role"], "assistant");
        assert_eq!(out[1]["content"], "Let me check.");
        let tc = out[1]["tool_calls"].as_array().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0]["id"], "t1");
        assert_eq!(tc[0]["function"]["name"], "bash");
    }

    #[test]
    fn test_translate_messages_tool_result() {
        let msgs = vec![
            Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "glob".into(),
                    input: json!({}),
                    thought_signature: None,
                }]),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "file1.rs\nfile2.rs".into(),
                    is_error: None,
                }]),
            },
        ];
        let out = translate_messages_to_oai("", &msgs);
        // placeholder user first (Gemini), then assistant + tool
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], GEMINI_MINIMAL_PARTS);
        assert_eq!(out[1]["role"], "assistant");
        assert_eq!(out[2]["role"], "tool");
        assert_eq!(out[2]["tool_call_id"], "t1");
        assert_eq!(out[2]["content"], "file1.rs\nfile2.rs");
    }

    #[test]
    fn test_translate_messages_tool_result_error() {
        let msgs = vec![
            Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "glob".into(),
                    input: json!({}),
                    thought_signature: None,
                }]),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "not found".into(),
                    is_error: Some(true),
                }]),
            },
        ];
        let out = translate_messages_to_oai("", &msgs);
        // Prepend placeholder user (Gemini): [user, assistant, tool]
        assert_eq!(out.len(), 3);
        assert_eq!(out[2]["role"], "tool");
        assert_eq!(out[2]["content"], "[Error] not found");
    }

    #[test]
    fn test_translate_messages_orphaned_tool_result_skipped() {
        // tool_result without matching tool_use should be stripped
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "orphan_id".into(),
                content: "stale result".into(),
                is_error: None,
            }]),
        }];
        let out = translate_messages_to_oai("", &msgs);
        assert!(out.is_empty());
    }

    #[test]
    fn test_translate_messages_image_block() {
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Image {
                    source: ImageSource {
                        source_type: "base64".into(),
                        media_type: "image/png".into(),
                        data: "AAAA".into(),
                    },
                },
                ContentBlock::Text {
                    text: "describe".into(),
                },
            ]),
        }];
        let out = translate_messages_to_oai("", &msgs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "user");
        let content = out[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "image_url");
        assert!(content[0]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "describe");
    }

    // -----------------------------------------------------------------------
    // translate_tools_to_oai
    // -----------------------------------------------------------------------

    #[test]
    fn test_translate_tools_to_oai() {
        let tools = vec![ToolDefinition {
            name: "bash".into(),
            description: "Run bash".into(),
            input_schema: json!({"type": "object", "properties": {"cmd": {"type": "string"}}}),
        }];
        let out = translate_tools_to_oai(&tools);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["type"], "function");
        assert_eq!(out[0]["function"]["name"], "bash");
        assert_eq!(out[0]["function"]["description"], "Run bash");
    }

    #[test]
    fn test_sanitize_oai_parameters_required_filtered() {
        // Gemini rejects required[] that are not in properties. Sanitizer should filter.
        let schema = json!({
            "type": "object",
            "properties": { "a": {"type": "string"}, "b": {"type": "integer"} },
            "required": ["a", "missing1", "b", "missing2"]
        });
        let out = sanitize_oai_parameters(&schema);
        let required = out["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert_eq!(required[0], "a");
        assert_eq!(required[1], "b");
    }

    #[test]
    fn test_sanitize_oai_parameters_no_properties_or_invalid() {
        // MCP etc can send schema without properties or with required not in properties; output safe schema.
        let schema = json!({
            "type": "object",
            "required": ["ghost1", "ghost2"]
        });
        let out = sanitize_oai_parameters(&schema);
        // When no valid required keys, we omit "required" or send empty.
        assert!(out.get("required").map(|r| r.as_array().unwrap().len()).unwrap_or(0) == 0);
        assert!(out.get("properties").is_some());
    }

    #[test]
    fn test_sanitize_oai_parameters_strips_enum_keeps_required() {
        // Schema with enum (e.g. write_tiered_memory) must still have required ⊆ properties
        // after sanitize, so Gemini/OpenRouter don't reject (required[0] not defined).
        let schema = json!({
            "type": "object",
            "properties": {
                "tier": { "type": "integer", "description": "Tier", "enum": [1, 2, 3] },
                "content": { "type": "string", "description": "Content" }
            },
            "required": ["tier", "content"]
        });
        let out = sanitize_oai_parameters(&schema);
        let props = out["properties"].as_object().unwrap();
        assert!(props.contains_key("tier"));
        assert!(props.contains_key("content"));
        assert!(props["tier"].get("enum").is_none(), "enum should be stripped");
        let required = out["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.iter().all(|r| props.contains_key(r.as_str().unwrap())));
    }

    // -----------------------------------------------------------------------
    // translate_oai_response
    // -----------------------------------------------------------------------

    #[test]
    fn test_translate_oai_response_text() {
        let oai = OaiResponse {
            choices: vec![OaiChoice {
                message: OaiMessage {
                    content: Some("Hello!".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(OaiUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
            }),
        };
        let resp = translate_oai_response(oai);
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ResponseContentBlock::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("Expected Text"),
        }
        let usage = resp.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
    }

    #[test]
    fn test_translate_oai_response_tool_calls() {
        let oai = OaiResponse {
            choices: vec![OaiChoice {
                message: OaiMessage {
                    content: None,
                    tool_calls: Some(vec![OaiToolCall {
                        id: "call_1".into(),
                        function: OaiFunction {
                            name: "bash".into(),
                            arguments: r#"{"command":"ls"}"#.into(),
                            thought_signature: None,
                        },
                        thought_signature: None,
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let resp = translate_oai_response(oai);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        match &resp.content[0] {
            ResponseContentBlock::ToolUse {
                id, name, input, ..
            } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "bash");
                assert_eq!(input["command"], "ls");
            }
            _ => panic!("Expected ToolUse"),
        }
    }

    #[test]
    fn test_translate_oai_response_empty_choices() {
        let oai = OaiResponse {
            choices: vec![],
            usage: None,
        };
        let resp = translate_oai_response(oai);
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
        match &resp.content[0] {
            ResponseContentBlock::Text { text } => assert_eq!(text, "(empty response)"),
            _ => panic!("Expected Text"),
        }
    }

    #[test]
    fn test_translate_oai_response_length_stop() {
        let oai = OaiResponse {
            choices: vec![OaiChoice {
                message: OaiMessage {
                    content: Some("partial".into()),
                    tool_calls: None,
                },
                finish_reason: Some("length".into()),
            }],
            usage: None,
        };
        let resp = translate_oai_response(oai);
        assert_eq!(resp.stop_reason.as_deref(), Some("max_tokens"));
    }

    #[test]
    fn test_translate_oai_response_text_and_tool_calls() {
        let oai = OaiResponse {
            choices: vec![OaiChoice {
                message: OaiMessage {
                    content: Some("thinking...".into()),
                    tool_calls: Some(vec![OaiToolCall {
                        id: "c1".into(),
                        function: OaiFunction {
                            name: "read_file".into(),
                            arguments: r#"{"path":"/tmp/x"}"#.into(),
                            thought_signature: None,
                        },
                        thought_signature: None,
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let resp = translate_oai_response(oai);
        assert_eq!(resp.content.len(), 2);
        match &resp.content[0] {
            ResponseContentBlock::Text { text } => assert_eq!(text, "thinking..."),
            _ => panic!("Expected Text"),
        }
        match &resp.content[1] {
            ResponseContentBlock::ToolUse { name, .. } => assert_eq!(name, "read_file"),
            _ => panic!("Expected ToolUse"),
        }
    }

    #[test]
    fn test_normalize_stop_reason_stream_variants() {
        assert_eq!(
            normalize_stop_reason(Some("tool_calls".into())).as_deref(),
            Some("tool_use")
        );
        assert_eq!(
            normalize_stop_reason(Some("length".into())).as_deref(),
            Some("max_tokens")
        );
        assert_eq!(
            normalize_stop_reason(Some("stop".into())).as_deref(),
            Some("end_turn")
        );
    }

    #[test]
    fn test_build_stream_response_tool_json_parsing() {
        let mut tool_blocks = std::collections::HashMap::new();
        tool_blocks.insert(
            0,
            StreamToolUseBlock {
                id: "call_1".into(),
                name: "bash".into(),
                input_json: r#"{"command":"ls","cwd":"/tmp"}"#.into(),
                thought_signature: None,
            },
        );
        let resp = build_stream_response(
            vec![0],
            std::collections::HashMap::new(),
            tool_blocks,
            Some("tool_use".into()),
            None,
        );
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        match &resp.content[0] {
            ResponseContentBlock::ToolUse {
                id, name, input, ..
            } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "bash");
                assert_eq!(input["command"], "ls");
                assert_eq!(input["cwd"], "/tmp");
            }
            _ => panic!("Expected ToolUse"),
        }
    }

    // -----------------------------------------------------------------------
    // create_provider
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_provider_anthropic() {
        let config = Config {
            telegram_bot_token: "tok".into(),
            bot_username: "bot".into(),
            llm_provider: "anthropic".into(),
            api_key: "key".into(),
            model: "claude-sonnet-4-5-20250929".into(),
            llm_base_url: None,
            max_tokens: 8192,
            max_tool_iterations: 100,
            max_history_messages: 50,
            max_document_size_mb: 100,
            workspace_dir: "/tmp".into(),
            openai_api_key: None,
            timezone: "UTC".into(),
            allowed_groups: vec![],
            control_chat_ids: vec![],
            max_session_messages: 40,
            compact_keep_recent: 20,
            whatsapp_access_token: None,
            whatsapp_phone_number_id: None,
            whatsapp_verify_token: None,
            whatsapp_webhook_port: 8080,
            discord_bot_token: None,
            discord_allowed_channels: vec![],
            show_thinking: false,
            web_enabled: false,
            web_host: "127.0.0.1".into(),
            web_port: 3900,
            web_auth_token: None,
            web_max_inflight_per_session: 2,
            web_max_requests_per_window: 8,
            web_rate_window_seconds: 10,
            web_run_history_limit: 512,
            web_session_idle_ttl_seconds: 300,
            browser_managed: false,
            browser_executable_path: None,
            browser_cdp_port_base: 9222,
            browser_idle_timeout_secs: None,
            browser_headless: false,
            agent_browser_path: None,
            cursor_agent_cli_path: "cursor-agent".into(),
            cursor_agent_model: String::new(),
            cursor_agent_timeout_secs: 600,
            social: None,
            vault: None,
            orchestrator_enabled: true,
            orchestrator_model: String::new(),
            tool_skill_agent_enabled: true,
            tool_skill_agent_model: String::new(),
            post_tool_evaluator_enabled: false,
            post_tool_evaluator_model: String::new(),
            delegate_tool_enabled: true,
            delegate_max_iterations: 10,
            delegate_model: String::new(),
            cursor_agent_tmux_session_prefix: "finally_a_value_bot-cursor".into(),
            cursor_agent_tmux_enabled: true,
            cursor_agent_runner_url: None,
        };
        // Should not panic
        let _provider = create_provider(&config);
    }

    #[test]
    fn test_create_provider_openai() {
        let config = Config {
            telegram_bot_token: "tok".into(),
            bot_username: "bot".into(),
            llm_provider: "openai".into(),
            api_key: "key".into(),
            model: "gpt-5.2".into(),
            llm_base_url: None,
            max_tokens: 8192,
            max_tool_iterations: 100,
            max_history_messages: 50,
            max_document_size_mb: 100,
            workspace_dir: "/tmp".into(),
            openai_api_key: None,
            timezone: "UTC".into(),
            allowed_groups: vec![],
            control_chat_ids: vec![],
            max_session_messages: 40,
            compact_keep_recent: 20,
            whatsapp_access_token: None,
            whatsapp_phone_number_id: None,
            whatsapp_verify_token: None,
            whatsapp_webhook_port: 8080,
            discord_bot_token: None,
            discord_allowed_channels: vec![],
            show_thinking: false,
            web_enabled: false,
            web_host: "127.0.0.1".into(),
            web_port: 3900,
            web_auth_token: None,
            web_max_inflight_per_session: 2,
            web_max_requests_per_window: 8,
            web_rate_window_seconds: 10,
            web_run_history_limit: 512,
            web_session_idle_ttl_seconds: 300,
            browser_managed: false,
            browser_executable_path: None,
            browser_cdp_port_base: 9222,
            browser_idle_timeout_secs: None,
            browser_headless: false,
            agent_browser_path: None,
            cursor_agent_cli_path: "cursor-agent".into(),
            cursor_agent_model: String::new(),
            cursor_agent_timeout_secs: 600,
            social: None,
            vault: None,
            orchestrator_enabled: true,
            orchestrator_model: String::new(),
            tool_skill_agent_enabled: true,
            tool_skill_agent_model: String::new(),
            post_tool_evaluator_enabled: false,
            post_tool_evaluator_model: String::new(),
            delegate_tool_enabled: true,
            delegate_max_iterations: 10,
            delegate_model: String::new(),
            cursor_agent_tmux_session_prefix: "finally_a_value_bot-cursor".into(),
            cursor_agent_tmux_enabled: true,
            cursor_agent_runner_url: None,
        };
        let _provider = create_provider(&config);
    }

    #[test]
    fn test_translate_messages_user_text_blocks_no_images_no_tool_results() {
        // User message with only text blocks (no images, no tool results) → plain text
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "first".into(),
                },
                ContentBlock::Text {
                    text: "second".into(),
                },
            ]),
        }];
        let out = translate_messages_to_oai("", &msgs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], "first\nsecond");
    }

    // -----------------------------------------------------------------------
    // sanitize_messages
    // -----------------------------------------------------------------------

    #[test]
    fn test_sanitize_messages_removes_orphaned_tool_results() {
        let msgs = vec![
            Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: json!({}),
                    thought_signature: None,
                }]),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "t1".into(),
                        content: "ok".into(),
                        is_error: None,
                    },
                    ContentBlock::ToolResult {
                        tool_use_id: "orphan".into(),
                        content: "stale".into(),
                        is_error: None,
                    },
                ]),
            },
        ];
        let sanitized = sanitize_messages(msgs);
        assert_eq!(sanitized.len(), 2);
        // The user message should only contain t1's result
        if let MessageContent::Blocks(blocks) = &sanitized[1].content {
            assert_eq!(blocks.len(), 1);
            if let ContentBlock::ToolResult { tool_use_id, .. } = &blocks[0] {
                assert_eq!(tool_use_id, "t1");
            } else {
                panic!("Expected ToolResult");
            }
        } else {
            panic!("Expected Blocks");
        }
    }

    #[test]
    fn test_sanitize_messages_drops_empty_user_message() {
        // User message with only orphaned tool_results → dropped entirely
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "orphan".into(),
                content: "stale".into(),
                is_error: None,
            }]),
        }];
        let sanitized = sanitize_messages(msgs);
        assert!(sanitized.is_empty());
    }

    #[test]
    fn test_sanitize_messages_preserves_text_messages() {
        let msgs = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("hello".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("hi".into()),
            },
        ];
        let sanitized = sanitize_messages(msgs);
        assert_eq!(sanitized.len(), 2);
    }

    #[test]
    fn test_sse_event_parser_multiline_data() {
        let mut parser = SseEventParser::default();
        let events = parser
            .push_chunk("event: message\n: keep-alive\ndata: {\"type\":\"x\",\ndata: \"v\":1}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], "{\"type\":\"x\",\n\"v\":1}");
    }

    #[test]
    fn test_sse_event_parser_finish_flushes_unterminated_event() {
        let mut parser = SseEventParser::default();
        let events = parser.push_chunk("data: hello");
        assert!(events.is_empty());
        let tail = parser.finish();
        assert_eq!(tail, vec!["hello".to_string()]);
    }

    // -----------------------------------------------------------------------
    // translate_messages_to_gemini
    // -----------------------------------------------------------------------

    #[test]
    fn test_translate_messages_to_gemini_text() {
        let msgs = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("hello".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("hi".into()),
            },
        ];
        let out = translate_messages_to_gemini(&msgs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["parts"][0]["text"], "hello");
        assert_eq!(out[1]["role"], "model");
        assert_eq!(out[1]["parts"][0]["text"], "hi");
    }

    #[test]
    fn test_translate_messages_to_gemini_tool_use_and_result() {
        let msgs = vec![
            Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: json!({"command": "ls"}),
                    thought_signature: Some("sig_abc".into()),
                }]),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "file1.txt\nfile2.txt".into(),
                    is_error: None,
                }]),
            },
        ];
        let out = translate_messages_to_gemini(&msgs);
        // Should have: user placeholder, assistant with tool_use, user with tool_result
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["role"], "user"); // placeholder
        assert_eq!(out[1]["role"], "model");
        let fc = &out[1]["parts"][0]["functionCall"];
        assert_eq!(fc["name"], "bash");
        assert_eq!(out[1]["parts"][0]["thoughtSignature"], "sig_abc");

        assert_eq!(out[2]["role"], "user");
        let fr = &out[2]["parts"][0]["functionResponse"];
        assert_eq!(fr["name"], "bash");
        assert_eq!(fr["response"]["result"], "file1.txt\nfile2.txt");
    }

    #[test]
    fn test_translate_messages_to_gemini_preserves_thought_signature() {
        let msgs = vec![Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "test"}),
                thought_signature: Some("preserved_sig".into()),
            }]),
        }];
        let out = translate_messages_to_gemini(&msgs);
        assert_eq!(out[1]["parts"][0]["thoughtSignature"], "preserved_sig");
    }

    // -----------------------------------------------------------------------
    // parse_gemini_response
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_gemini_response_text() {
        let response = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{ "text": "Hello, world!" }]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5
            }
        });
        let result = parse_gemini_response(&response.to_string()).unwrap();
        assert_eq!(result.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            ResponseContentBlock::Text { text } => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected Text"),
        }
        assert_eq!(result.usage.unwrap().input_tokens, 10);
    }

    #[test]
    fn test_parse_gemini_response_function_call() {
        let response = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "search_web",
                            "args": { "query": "rust programming" }
                        },
                        "thoughtSignature": "sig_xyz"
                    }]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 20,
                "candidatesTokenCount": 10
            }
        });
        let result = parse_gemini_response(&response.to_string()).unwrap();
        assert_eq!(result.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            ResponseContentBlock::ToolUse {
                id,
                name,
                input,
                thought_signature,
            } => {
                assert_eq!(name, "search_web");
                assert_eq!(input["query"], "rust programming");
                assert_eq!(thought_signature.as_deref(), Some("sig_xyz"));
                assert!(!id.is_empty()); // UUID generated
            }
            _ => panic!("Expected ToolUse"),
        }
    }

    #[test]
    fn test_parse_gemini_response_text_and_function_call() {
        let response = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        { "text": "Let me search for that." },
                        {
                            "functionCall": {
                                "name": "search",
                                "args": { "q": "test" }
                            },
                            "thoughtSignature": "sig_abc"
                        }
                    ]
                },
                "finishReason": "STOP"
            }]
        });
        let result = parse_gemini_response(&response.to_string()).unwrap();
        assert_eq!(result.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(result.content.len(), 2);
        match &result.content[0] {
            ResponseContentBlock::Text { text } => {
                assert_eq!(text, "Let me search for that.")
            }
            _ => panic!("Expected Text"),
        }
        match &result.content[1] {
            ResponseContentBlock::ToolUse { name, .. } => assert_eq!(name, "search"),
            _ => panic!("Expected ToolUse"),
        }
    }

    #[test]
    fn test_parse_gemini_response_empty() {
        let response = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": []
                },
                "finishReason": "STOP"
            }]
        });
        let result = parse_gemini_response(&response.to_string()).unwrap();
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            ResponseContentBlock::Text { text } => assert_eq!(text, ""),
            _ => panic!("Expected Text"),
        }
    }

    #[test]
    fn test_create_provider_google() {
        let config = Config {
            telegram_bot_token: "tok".into(),
            bot_username: "bot".into(),
            llm_provider: "google".into(),
            api_key: "key".into(),
            model: "gemini-2.5-flash".into(),
            llm_base_url: None,
            max_tokens: 8192,
            max_tool_iterations: 100,
            max_history_messages: 50,
            max_document_size_mb: 100,
            workspace_dir: "/tmp".into(),
            openai_api_key: None,
            timezone: "UTC".into(),
            allowed_groups: vec![],
            control_chat_ids: vec![],
            max_session_messages: 40,
            compact_keep_recent: 20,
            whatsapp_access_token: None,
            whatsapp_phone_number_id: None,
            whatsapp_verify_token: None,
            whatsapp_webhook_port: 8080,
            discord_bot_token: None,
            discord_allowed_channels: vec![],
            show_thinking: false,
            web_enabled: false,
            web_host: "127.0.0.1".into(),
            web_port: 3900,
            web_auth_token: None,
            web_max_inflight_per_session: 2,
            web_max_requests_per_window: 8,
            web_rate_window_seconds: 10,
            web_run_history_limit: 512,
            web_session_idle_ttl_seconds: 300,
            browser_managed: false,
            browser_executable_path: None,
            browser_cdp_port_base: 9222,
            browser_idle_timeout_secs: None,
            browser_headless: false,
            agent_browser_path: None,
            cursor_agent_cli_path: "cursor-agent".into(),
            cursor_agent_model: String::new(),
            cursor_agent_timeout_secs: 600,
            social: None,
            vault: None,
            orchestrator_enabled: true,
            orchestrator_model: String::new(),
            tool_skill_agent_enabled: true,
            tool_skill_agent_model: String::new(),
            post_tool_evaluator_enabled: false,
            post_tool_evaluator_model: String::new(),
            delegate_tool_enabled: true,
            delegate_max_iterations: 10,
            delegate_model: String::new(),
            cursor_agent_tmux_session_prefix: "finally_a_value_bot-cursor".into(),
            cursor_agent_tmux_enabled: true,
            cursor_agent_runner_url: None,
        };
        // Should not panic
        let _provider = create_provider(&config);
    }
}
