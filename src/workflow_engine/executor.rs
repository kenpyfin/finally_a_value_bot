use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use crate::config::Config;
use crate::db::Database;
use crate::hook_runtime::{run_hooks_for_event_async, HookEventName, HookRunInput};
use crate::safety_redaction::EnvSecretRedactor;
use crate::tools::{inject_auth_context, ToolAuthContext, ToolRegistry, ToolResult};

use super::context::WorkflowExecutionContext;
use super::schema::{WorkflowDefinition, WorkflowRunReport, WorkflowStep, WorkflowStepOutcome};
use super::template::{render_json_value, render_template};

pub struct WorkflowExecutionParams<'a> {
    pub config: &'a Config,
    pub db: Arc<Database>,
    pub tools: &'a ToolRegistry,
    pub env_redactor: Arc<EnvSecretRedactor>,
    pub auth: &'a ToolAuthContext,
    pub chat_id: i64,
    pub persona_id: i64,
    pub caller_channel: &'a str,
    pub is_scheduled_task: bool,
    pub run_key: Option<&'a str>,
    pub max_steps: usize,
}

pub async fn execute_workflow(
    def: &WorkflowDefinition,
    inputs: serde_json::Value,
    params: WorkflowExecutionParams<'_>,
) -> WorkflowRunReport {
    if !def.enabled {
        return WorkflowRunReport {
            workflow_id: def.id.clone(),
            success: false,
            steps: Vec::new(),
            deliver_text: String::new(),
            error: Some("workflow is disabled".into()),
        };
    }

    if def.steps.len() > params.max_steps {
        return WorkflowRunReport {
            workflow_id: def.id.clone(),
            success: false,
            steps: Vec::new(),
            deliver_text: String::new(),
            error: Some(format!(
                "workflow has {} steps; maximum allowed is {}",
                def.steps.len(),
                params.max_steps
            )),
        };
    }

    let workflow_id_owned = def.id.clone();
    let chat_id = params.chat_id;
    let persona_id = params.persona_id;

    if let Some(run_key) = params.run_key {
        let _ = crate::db::call_blocking(params.db.clone(), {
            let run_key = run_key.to_string();
            let workflow_id = workflow_id_owned.clone();
            move |db| {
                db.append_run_timeline_event(
                    &run_key,
                    chat_id,
                    persona_id,
                    "workflow_start",
                    Some(&format!(
                        r#"{{"workflow_id":"{}"}}"#,
                        workflow_id.replace('"', "'")
                    )),
                )
            }
        })
        .await;
    }

    let mut ctx = WorkflowExecutionContext::new(inputs);
    let mut outcomes = Vec::new();
    let mut deliver_text = String::new();
    let mut fatal_error: Option<String> = None;

    for (index, step) in def.steps.iter().enumerate() {
        if let Some(run_key) = params.run_key {
            let step_id = step.id();
            let _ = crate::db::call_blocking(params.db.clone(), {
                let run_key = run_key.to_string();
                let step_id = step_id.to_string();
                let workflow_id = workflow_id_owned.clone();
                move |db| {
                    db.append_run_timeline_event(
                        &run_key,
                        chat_id,
                        persona_id,
                        "workflow_step_start",
                        Some(&format!(
                            r#"{{"workflow_id":"{}","step_id":"{}","step_index":{}}}"#,
                            workflow_id.replace('"', "'"),
                            step_id.replace('"', "'"),
                            index
                        )),
                    )
                }
            })
            .await;
        }

        let started = Instant::now();
        let template_ctx = ctx.as_template_value();
        let outcome = match step {
            WorkflowStep::Set { id, var, value } => {
                let rendered = render_template(value, &template_ctx);
                ctx.set_var(var, rendered.clone());
                WorkflowStepOutcome {
                    id: id.clone(),
                    step_type: "set".into(),
                    is_error: false,
                    stdout: rendered,
                    duration_ms: started.elapsed().as_millis(),
                    error_type: None,
                }
            }
            WorkflowStep::Deliver {
                id,
                text,
                send_message,
                message_input,
            } => {
                let rendered = render_template(text, &template_ctx);
                deliver_text = rendered.clone();
                let mut is_error = false;
                let mut stdout = rendered.clone();
                let mut error_type = None;

                if *send_message {
                    let mut msg_input = message_input
                        .clone()
                        .unwrap_or_else(|| json!({ "text": rendered }));
                    if let Ok(rendered_input) = render_json_value(&msg_input, &template_ctx) {
                        msg_input = rendered_input;
                    }
                    if msg_input.get("text").is_none() {
                        if let Some(obj) = msg_input.as_object_mut() {
                            obj.insert("text".into(), json!(rendered));
                        }
                    }
                    let tool_result =
                        execute_tool_step(&params, "send_message", msg_input, None).await;
                    is_error = tool_result.is_error;
                    stdout = tool_result.content.clone();
                    error_type = tool_result.error_type.clone();
                }

                WorkflowStepOutcome {
                    id: id.clone(),
                    step_type: "deliver".into(),
                    is_error,
                    stdout,
                    duration_ms: started.elapsed().as_millis(),
                    error_type,
                }
            }
            WorkflowStep::Tool {
                id,
                tool,
                input,
                timeout_secs,
            } => {
                let rendered_input = render_json_value(input, &template_ctx)
                    .unwrap_or_else(|e| json!({ "error": e }));
                let tool_result =
                    execute_tool_step(&params, tool, rendered_input, *timeout_secs).await;
                step_outcome_from_tool(id, "tool", tool_result, started)
            }
            WorkflowStep::Script {
                id,
                skill_name,
                script,
                args,
                interpreter,
                timeout_secs,
            } => {
                let rendered_args: Vec<String> = args
                    .iter()
                    .map(|a| render_template(a, &template_ctx))
                    .collect();
                let mut input = json!({
                    "skill_name": skill_name,
                    "script": script,
                    "args": rendered_args,
                });
                if let Some(interp) = interpreter {
                    input["interpreter"] = json!(interp);
                }
                if let Some(t) = timeout_secs {
                    input["timeout_secs"] = json!(t);
                }
                let tool_result =
                    execute_tool_step(&params, "run_skill_script", input, *timeout_secs).await;
                step_outcome_from_tool(id, "script", tool_result, started)
            }
            WorkflowStep::Bash {
                id,
                command,
                timeout_secs,
            } => {
                let rendered_cmd = render_template(command, &template_ctx);
                let mut input = json!({ "command": rendered_cmd });
                if let Some(t) = timeout_secs {
                    input["timeout_secs"] = json!(t);
                }
                let tool_result = execute_tool_step(&params, "bash", input, *timeout_secs).await;
                step_outcome_from_tool(id, "bash", tool_result, started)
            }
        };

        ctx.record_step(step.id(), &outcome);
        outcomes.push(outcome.clone());

        if let Some(run_key) = params.run_key {
            let step_id = step.id();
            let is_error = outcome.is_error;
            let _ = crate::db::call_blocking(params.db.clone(), {
                let run_key = run_key.to_string();
                let step_id = step_id.to_string();
                let workflow_id = workflow_id_owned.clone();
                move |db| {
                    db.append_run_timeline_event(
                        &run_key,
                        chat_id,
                        persona_id,
                        "workflow_step_end",
                        Some(&format!(
                            r#"{{"workflow_id":"{}","step_id":"{}","step_index":{},"is_error":{}}}"#,
                            workflow_id.replace('"', "'"),
                            step_id.replace('"', "'"),
                            index,
                            is_error
                        )),
                    )
                }
            })
            .await;
        }

        if outcome.is_error && def.on_error == "fail" {
            fatal_error = Some(format!(
                "step '{}' failed: {}",
                outcome.id,
                truncate(&outcome.stdout, 2000)
            ));
            break;
        }
    }

    let success = fatal_error.is_none() && !outcomes.iter().any(|o| o.is_error);
    let event_type = if success {
        "workflow_complete"
    } else {
        "workflow_failed"
    };
    if let Some(run_key) = params.run_key {
        let _ = crate::db::call_blocking(params.db.clone(), {
            let run_key = run_key.to_string();
            let err = fatal_error.clone();
            let workflow_id = workflow_id_owned.clone();
            move |db| {
                db.append_run_timeline_event(
                    &run_key,
                    chat_id,
                    persona_id,
                    event_type,
                    Some(&format!(
                        r#"{{"workflow_id":"{}","success":{},"error":{}}}"#,
                        workflow_id.replace('"', "'"),
                        success,
                        err.map(|e| format!("\"{}\"", e.replace('"', "'")))
                            .unwrap_or_else(|| "null".into())
                    )),
                )
            }
        })
        .await;
    }

    WorkflowRunReport {
        workflow_id: def.id.clone(),
        success,
        steps: outcomes,
        deliver_text,
        error: fatal_error,
    }
}

fn step_outcome_from_tool(
    id: &str,
    step_type: &str,
    tool_result: ToolResult,
    started: Instant,
) -> WorkflowStepOutcome {
    WorkflowStepOutcome {
        id: id.to_string(),
        step_type: step_type.to_string(),
        is_error: tool_result.is_error,
        stdout: tool_result.content,
        duration_ms: started.elapsed().as_millis(),
        error_type: tool_result.error_type,
    }
}

async fn execute_tool_step(
    params: &WorkflowExecutionParams<'_>,
    tool_name: &str,
    input: serde_json::Value,
    timeout_secs: Option<u64>,
) -> ToolResult {
    let mut hook_input = inject_auth_context(input, params.auth);

    let pre = run_hooks_for_event_async(
        params.db.clone(),
        params.config,
        params.env_redactor.as_ref(),
        HookEventName::PreToolUse,
        &HookRunInput {
            chat_id: params.chat_id,
            persona_id: params.persona_id,
            caller_channel: params.caller_channel.to_string(),
            is_scheduled_task: params.is_scheduled_task,
            tool_name: Some(tool_name.to_string()),
            tool_input: Some(hook_input.clone()),
            ..HookRunInput::default()
        },
    )
    .await
    .ok();
    if let Some(hook) = pre.as_ref() {
        if let Some(reason) = hook.blocked_reason.as_deref() {
            return ToolResult::error(format!("[Tool blocked by hook] {reason}"))
                .with_error_type("hook_block");
        }
        if let Some(updated) = hook.updated_tool_input.clone() {
            hook_input = updated;
        }
    }

    let timeout = timeout_secs.unwrap_or(3600);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout),
        params
            .tools
            .execute_with_auth(tool_name, hook_input, params.auth),
    )
    .await;

    let tool_result = match result {
        Ok(r) => r,
        Err(_) => ToolResult::error(format!(
            "Tool '{tool_name}' timed out after {timeout}s during workflow execution"
        ))
        .with_error_type("timeout"),
    };

    let _ = run_hooks_for_event_async(
        params.db.clone(),
        params.config,
        params.env_redactor.as_ref(),
        HookEventName::PostToolUse,
        &HookRunInput {
            chat_id: params.chat_id,
            persona_id: params.persona_id,
            caller_channel: params.caller_channel.to_string(),
            is_scheduled_task: params.is_scheduled_task,
            tool_name: Some(tool_name.to_string()),
            tool_output: Some(tool_result.content.clone()),
            tool_is_error: Some(tool_result.is_error),
            ..HookRunInput::default()
        },
    )
    .await;

    tool_result
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
