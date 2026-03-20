use std::sync::Arc;

use tracing::{error, info};

use crate::claude::{Message, MessageContent, ResponseContentBlock};
use crate::channel::deliver_to_contact;
use crate::db::call_blocking;
use crate::telegram::{process_with_agent, AgentRequestContext, AppState};

/// Spawn a background job and deliver the final result asynchronously.
pub fn spawn_background_job(
    state: Arc<AppState>,
    job_id: String,
    chat_id: i64,
    persona_id: i64,
    prompt: String,
) {
    tokio::spawn(async move {
        info!(
            job_id = %job_id,
            chat_id = chat_id,
            "Background job starting"
        );

        // Mark running
        let jid = job_id.clone();
        if let Err(e) = call_blocking(state.db.clone(), move |db| {
            db.mark_background_job_running(&jid)
        })
        .await
        {
            error!(job_id = %job_id, "Failed to mark background job running: {e}");
            return;
        }

        // Run the agent with is_background_job=true (disables further handoff)
        let bg_result = process_with_agent(
            &state,
            AgentRequestContext {
                caller_channel: "web",
                chat_id,
                chat_type: "private",
                persona_id,
                is_scheduled_task: false,
                is_background_job: true,
            },
            Some(&prompt),
            None,
        )
        .await;

        let (raw_output, raw_success) = match bg_result {
            Ok(text) => (text, true),
            Err(e) => (format!("Background job error: {e}"), false),
        };

        // Persist raw background result/error.
        let jid = job_id.clone();
        let output_for_db = raw_output.clone();
        let _ = call_blocking(state.db.clone(), move |db| {
            db.mark_background_job_completed_raw(&jid, &output_for_db)
        })
        .await;

        // Mark continuation lane while we generate user-facing output.
        let jid = job_id.clone();
        let _ = call_blocking(state.db.clone(), move |db| {
            db.mark_background_job_main_agent_processing(&jid)
        })
        .await;

        let format_prompt = if raw_success {
            format!(
                "The user's original request was: \"{}\".\n\nBackground job result:\n{}\n\nRespond to the user with a concise final answer.",
                prompt, raw_output
            )
        } else {
            format!(
                "The user's original request was: \"{}\".\n\nBackground job error:\n{}\n\nInform the user about this failure and suggest next steps.",
                prompt, raw_output
            )
        };

        let final_result = state
            .llm
            .send_message(
                "You are a concise assistant writing final user-facing replies.",
                vec![Message {
                    role: "user".into(),
                    content: MessageContent::Text(format_prompt),
                }],
                None,
            )
            .await
            .map(|resp| {
                resp.content
                    .iter()
                    .filter_map(|block| match block {
                        ResponseContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("")
            });

        match final_result {
            Ok(final_text) => {
                info!(
                    job_id = %job_id,
                    chat_id = chat_id,
                    response_len = final_text.len(),
                    "Background job: main agent produced final response"
                );
                if let Err(e) = deliver_to_contact(
                    state.db.clone(),
                    Some(&state.bot),
                    state.discord_http.as_deref(),
                    &state.config.bot_username,
                    chat_id,
                    persona_id,
                    &final_text,
                    Some(state.config.workspace_root_absolute()),
                )
                .await
                {
                    error!(
                        job_id = %job_id,
                        "Background job: failed to deliver final response: {e}"
                    );
                    let jid = job_id.clone();
                    let err_text = format!("Delivery failed after continuation: {e}");
                    let _ = call_blocking(state.db.clone(), move |db| {
                        db.fail_background_job(&jid, &err_text)
                    })
                    .await;
                    info!(job_id = %job_id, chat_id = chat_id, "Background job finished");
                    return;
                }

                let jid = job_id.clone();
                let _ = call_blocking(state.db.clone(), move |db| {
                    db.mark_background_job_done(&jid)
                })
                .await;
            }
            Err(e) => {
                error!(
                    job_id = %job_id,
                    "Background job: format pass failed: {e}"
                );
                let fallback = if raw_success {
                    format!("Your background task completed, but I had trouble generating a summary. Here is the raw result:\n\n{}", raw_output)
                } else {
                    format!("Your background task failed: {}", raw_output)
                };
                let _ = deliver_to_contact(
                    state.db.clone(),
                    Some(&state.bot),
                    state.discord_http.as_deref(),
                    &state.config.bot_username,
                    chat_id,
                    persona_id,
                    &fallback,
                    Some(state.config.workspace_root_absolute()),
                )
                .await;

                let jid = job_id.clone();
                let err_text = format!("Background formatting failed: {e}");
                let _ = call_blocking(state.db.clone(), move |db| {
                    db.fail_background_job(&jid, &err_text)
                })
                .await;
            }
        }

        info!(job_id = %job_id, chat_id = chat_id, "Background job finished");
    });
}
