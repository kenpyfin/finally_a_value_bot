use std::sync::Arc;

use tracing::{error, info};

use crate::channel::deliver_to_contact;
use crate::db::call_blocking;
use crate::telegram::{process_with_agent, AgentRequestContext, AppState};

/// Spawn a background job that re-runs the user prompt with extended timeouts.
/// When the background run completes (success or failure), the raw result is fed
/// back through the main agent as tool-context so the main agent produces the
/// final user-facing reply.
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

        // Feed the background result back to the main agent as tool-context so
        // the main agent can reason about it and produce the final user reply.
        // Mark continuation lane and ask main agent to produce user-facing output.
        let jid = job_id.clone();
        let _ = call_blocking(state.db.clone(), move |db| {
            db.mark_background_job_main_agent_processing(&jid)
        })
        .await;

        let continuation_prompt = if raw_success {
            format!(
                "[System: A background job completed for this chat. The user's original request was: \"{}\". The background job produced the following result. Please review it and respond to the user with the final answer.]\n\n{}",
                prompt, raw_output
            )
        } else {
            format!(
                "[System: A background job failed for this chat. The user's original request was: \"{}\". The error was: {}. Please inform the user about this failure and suggest next steps.]",
                prompt, raw_output
            )
        };

        let final_result = process_with_agent(
            &state,
            AgentRequestContext {
                caller_channel: "web",
                chat_id,
                chat_type: "private",
                persona_id,
                is_scheduled_task: false,
                is_background_job: true,
            },
            Some(&continuation_prompt),
            None,
        )
        .await;

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
                    "Background job: main agent continuation failed: {e}"
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
                )
                .await;

                let jid = job_id.clone();
                let err_text = format!("Main-agent continuation failed: {e}");
                let _ = call_blocking(state.db.clone(), move |db| {
                    db.fail_background_job(&jid, &err_text)
                })
                .await;
            }
        }

        info!(job_id = %job_id, chat_id = chat_id, "Background job finished");
    });
}
