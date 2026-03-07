//! Ephemeral agent loop: a reusable tool-use loop with no session persistence, TSA, or PTE.
//! Used by the delegate tool and potentially other sub-agent invocations.

use crate::claude::{ContentBlock, Message, MessageContent, ResponseContentBlock};
use crate::error::MicroClawError;
use crate::llm::LlmProvider;
use crate::tools::{ToolAuthContext, ToolRegistry};
use tracing::info;

/// Run an ephemeral agent loop. Returns the final assistant text or an error.
///
/// This is a simplified version of the main agent loop: it iterates LLM calls,
/// executes tool calls, and returns when the LLM produces an `end_turn` response
/// or the iteration cap is reached. No session save, no TSA gating, no PTE evaluation.
pub async fn run_ephemeral_loop(
    llm: &dyn LlmProvider,
    tools: &ToolRegistry,
    system_prompt: &str,
    mut messages: Vec<Message>,
    auth: &ToolAuthContext,
    max_iterations: usize,
    llm_timeout_secs: u64,
    tool_timeout_secs: u64,
) -> Result<String, MicroClawError> {
    let tool_defs = tools.definitions();
    let tool_names: Vec<String> = tool_defs.iter().map(|d| d.name.clone()).collect();
    info!(
        "Sub-agent starting: max_iterations={}, tools=[{}], system_prompt_len={}",
        max_iterations,
        tool_names.join(", "),
        system_prompt.len()
    );

    for iteration in 0..max_iterations {
        info!(
            "Sub-agent iteration {}/{}: sending LLM request ({} messages in context)",
            iteration + 1,
            max_iterations,
            messages.len()
        );

        let llm_start = std::time::Instant::now();
        let response = match tokio::time::timeout(
            std::time::Duration::from_secs(llm_timeout_secs),
            llm.send_message(system_prompt, messages.clone(), Some(tool_defs.clone())),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                info!(
                    "Sub-agent iteration {}/{}: LLM error after {}ms: {}",
                    iteration + 1,
                    max_iterations,
                    llm_start.elapsed().as_millis(),
                    e
                );
                return Err(e);
            }
            Err(_) => {
                info!(
                    "Sub-agent iteration {}/{}: LLM timed out after {}s",
                    iteration + 1,
                    max_iterations,
                    llm_timeout_secs
                );
                return Err(MicroClawError::Config(format!(
                    "Sub-agent LLM call timed out after {}s (iteration {})",
                    llm_timeout_secs,
                    iteration + 1
                )));
            }
        };

        let stop_reason = response.stop_reason.as_deref().unwrap_or("end_turn");

        // Extract any assistant text from this response
        let assistant_text: String = response
            .content
            .iter()
            .filter_map(|block| match block {
                ResponseContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        let assistant_text_preview = if assistant_text.len() > 200 {
            format!("{}...", &assistant_text[..assistant_text.floor_char_boundary(200)])
        } else {
            assistant_text.clone()
        };

        info!(
            "Sub-agent iteration {}/{}: stop_reason={}, llm_ms={}, text_len={}, text_preview=\"{}\"",
            iteration + 1,
            max_iterations,
            stop_reason,
            llm_start.elapsed().as_millis(),
            assistant_text.len(),
            assistant_text_preview.replace('\n', "\\n")
        );

        if stop_reason == "end_turn" || stop_reason == "max_tokens" {
            info!(
                "Sub-agent finished: stop_reason={}, final_response_len={}, total_iterations={}",
                stop_reason,
                assistant_text.len(),
                iteration + 1
            );
            return Ok(assistant_text);
        }

        if stop_reason == "tool_use" {
            let tool_calls: Vec<(&str, &serde_json::Value)> = response
                .content
                .iter()
                .filter_map(|block| match block {
                    ResponseContentBlock::ToolUse { name, input, .. } => {
                        Some((name.as_str(), input))
                    }
                    _ => None,
                })
                .collect();

            info!(
                "Sub-agent iteration {}/{}: {} tool call(s): [{}]",
                iteration + 1,
                max_iterations,
                tool_calls.len(),
                tool_calls
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let assistant_content: Vec<ContentBlock> = response
                .content
                .iter()
                .map(|block| match block {
                    ResponseContentBlock::Text { text } => ContentBlock::Text {
                        text: text.clone(),
                    },
                    ResponseContentBlock::ToolUse {
                        id,
                        name,
                        input,
                        thought_signature,
                    } => ContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                        thought_signature: thought_signature.clone(),
                    },
                })
                .collect();

            messages.push(Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(assistant_content),
            });

            let mut tool_results = Vec::new();

            for block in &response.content {
                if let ResponseContentBlock::ToolUse { id, name, input, .. } = block {
                    let input_str = serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
                    let input_preview = if input_str.len() > 300 {
                        format!("{}...", &input_str[..300])
                    } else {
                        input_str
                    };
                    info!(
                        "Sub-agent iteration {}/{}: executing tool={}, input={}",
                        iteration + 1,
                        max_iterations,
                        name,
                        input_preview
                    );

                    let tool_start = std::time::Instant::now();
                    let result = match tokio::time::timeout(
                        std::time::Duration::from_secs(tool_timeout_secs),
                        tools.execute_with_auth(name, input.clone(), auth),
                    )
                    .await
                    {
                        Ok(tool_result) => tool_result,
                        Err(_) => {
                            info!(
                                "Sub-agent iteration {}/{}: tool={} TIMED OUT after {}s",
                                iteration + 1,
                                max_iterations,
                                name,
                                tool_timeout_secs
                            );
                            crate::tools::ToolResult::error(format!(
                                "Tool execution timed out after {}s.",
                                tool_timeout_secs
                            ))
                        }
                    };

                    let result_preview = if result.content.len() > 300 {
                        format!(
                            "{}...",
                            &result.content[..result.content.floor_char_boundary(300)]
                        )
                    } else {
                        result.content.clone()
                    };
                    info!(
                        "Sub-agent iteration {}/{}: tool={} {}completed in {}ms, result_len={}, is_error={}, preview=\"{}\"",
                        iteration + 1,
                        max_iterations,
                        name,
                        if result.is_error { "FAILED " } else { "" },
                        tool_start.elapsed().as_millis(),
                        result.content.len(),
                        result.is_error,
                        result_preview.replace('\n', "\\n")
                    );

                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: result.content,
                        is_error: if result.is_error { Some(true) } else { None },
                    });
                }
            }

            messages.push(Message {
                role: "user".into(),
                content: MessageContent::Blocks(tool_results),
            });

            continue;
        }

        // Unknown stop reason — extract any text
        info!(
            "Sub-agent iteration {}/{}: unknown stop_reason={}, returning text ({} chars)",
            iteration + 1,
            max_iterations,
            stop_reason,
            assistant_text.len()
        );
        return Ok(assistant_text);
    }

    info!(
        "Sub-agent reached max iterations ({}), stopping",
        max_iterations
    );
    Ok("Sub-agent reached maximum iterations.".to_string())
}
