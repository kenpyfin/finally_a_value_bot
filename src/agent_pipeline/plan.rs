//! Workflow / plan resolution for the deterministic pipeline.

use serde::{Deserialize, Serialize};

use crate::claude::{Message, MessageContent, ResponseContentBlock};
use crate::error::FinallyAValueBotError;
use crate::memory::{PersonaMemoryState, SopPointer};
use crate::multimodel::ModelTier;
use crate::telegram::AppState;

use super::intent::IntentDecision;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub goal: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub inputs: String,
    #[serde(default)]
    pub expected_output: String,
    #[serde(default)]
    pub verification: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub source: String,
    #[serde(default)]
    pub vault_path: Option<String>,
    pub steps: Vec<PlanStep>,
}

const PLAN_SYSTEM: &str = "\
You are a task planner. Given a user goal, produce a JSON execution plan only (no markdown fences).\n\
Schema: {\"source\":\"ephemeral\",\"steps\":[{\"id\":\"1\",\"goal\":\"...\",\
\"allowed_tools\":[\"bash\",\"read_file\"],\"inputs\":\"context\",\"expected_output\":\"...\",\
\"verification\":\"how to verify success\"}]}\n\
Rules: 3-8 steps max; each step one clear outcome; allowed_tools from common agent tools \
(bash, read_file, grep, glob, search_vault, activate_skill, run_skill_script, write_file, edit_file); \
verification is a short pass/fail criterion.\n\
If a REFERENCE SOP is provided, treat it as the canonical procedure for its domain, but include \
ONLY the steps actually required to satisfy the CURRENT request. Do not blindly expand every SOP \
stage into a step: for a display/read/status request, skip generation, editing, publishing, or \
scheduling stages and plan just the minimal read + present steps. When a step follows an SOP stage, \
name that stage in the step's inputs field.";

/// Maximum characters of an SOP body injected into the planner prompt as reference.
const SOP_REFERENCE_MAX_CHARS: usize = 6000;

/// A vault SOP matched for the current request, passed to the planner as reference context.
struct SopReference {
    id: String,
    vault_path: String,
    body: String,
}

pub async fn resolve_plan(
    state: &AppState,
    intent: &IntentDecision,
    memory_state: Option<&PersonaMemoryState>,
    user_request: &str,
) -> Result<Plan, FinallyAValueBotError> {
    let sop_ref = find_sop_reference(state, intent, memory_state).await?;
    generate_plan(state, intent, user_request, sop_ref.as_ref()).await
}

async fn find_sop_reference(
    state: &AppState,
    intent: &IntentDecision,
    memory_state: Option<&PersonaMemoryState>,
) -> Result<Option<SopReference>, FinallyAValueBotError> {
    let candidates = collect_sop_candidates(intent, memory_state);
    for sop in candidates {
        if let Some(body) = read_vault_sop_body(state, &sop.vault_path).await? {
            if body.trim().len() >= 80 {
                return Ok(Some(SopReference {
                    id: sop.id.clone(),
                    vault_path: sop.vault_path.clone(),
                    body,
                }));
            }
        }
    }
    Ok(None)
}

fn collect_sop_candidates(
    intent: &IntentDecision,
    memory_state: Option<&PersonaMemoryState>,
) -> Vec<SopPointer> {
    let mut out: Vec<SopPointer> = Vec::new();
    if let Some(hint) = intent.candidate_sop_hint.as_deref() {
        let hint = hint.trim();
        if hint.starts_with("ORIGIN/") {
            out.push(SopPointer {
                id: "hint".into(),
                vault_path: hint.into(),
                summary: String::new(),
            });
        }
    }
    if let Some(ms) = memory_state {
        out.extend(ms.tier2.sops.clone());
    }
    out
}

async fn read_vault_sop_body(
    state: &AppState,
    vault_path: &str,
) -> Result<Option<String>, FinallyAValueBotError> {
    let ws = state.config.workspace_root_absolute();
    let full = ws.join(vault_path.trim_start_matches('/'));
    if !full.is_file() {
        let shared = ws.join("shared").join(vault_path.trim_start_matches('/'));
        if shared.is_file() {
            return Ok(Some(
                tokio::fs::read_to_string(&shared).await.unwrap_or_default(),
            ));
        }
        return Ok(None);
    }
    Ok(Some(
        tokio::fs::read_to_string(&full).await.unwrap_or_default(),
    ))
}

fn default_tool_allowlist() -> Vec<String> {
    vec![
        "bash".into(),
        "read_file".into(),
        "grep".into(),
        "glob".into(),
        "search_vault".into(),
        "activate_skill".into(),
        "run_skill_script".into(),
        "write_file".into(),
        "edit_file".into(),
        "web_search".into(),
        "web_fetch".into(),
    ]
}

async fn generate_plan(
    state: &AppState,
    intent: &IntentDecision,
    user_request: &str,
    sop_ref: Option<&SopReference>,
) -> Result<Plan, FinallyAValueBotError> {
    let criteria = if intent.success_criteria.is_empty() {
        "Infer reasonable success criteria.".into()
    } else {
        intent.success_criteria.join("; ")
    };
    let sop_block = match sop_ref {
        Some(sop) => format!(
            "\n\nREFERENCE SOP ({path}):\n{body}\n\
             Use the SOP for canonical procedures, but plan ONLY the steps required for the \
             current request; skip unrelated SOP stages.",
            path = sop.vault_path,
            body = truncate(&sop.body, SOP_REFERENCE_MAX_CHARS),
        ),
        None => String::new(),
    };
    let user_msg = format!(
        "Goal: {}\nSuccess criteria: {}\nUser request:\n{}{}\n\nRespond with JSON plan only.",
        intent.restated_goal,
        criteria,
        user_request.chars().take(4000).collect::<String>(),
        sop_block,
    );
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user_msg),
    }];
    let response = state
        .llm
        .send_message_for_tier(ModelTier::Strategy, PLAN_SYSTEM, messages, None)
        .await?;
    let text: String = response
        .content
        .iter()
        .filter_map(|b| match b {
            ResponseContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    let mut plan = parse_plan_json(&text).unwrap_or_else(|_| fallback_single_step_plan(intent));
    if let Some(sop) = sop_ref {
        plan.source = format!("vault_sop_scoped:{}", sop.id);
        plan.vault_path = Some(sop.vault_path.clone());
    }
    Ok(plan)
}

fn parse_plan_json(text: &str) -> Result<Plan, FinallyAValueBotError> {
    let trimmed = text.trim();
    let json_str = if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    let mut plan: Plan = serde_json::from_str(json_str)
        .map_err(|e| FinallyAValueBotError::Config(format!("plan JSON parse: {e}")))?;
    if plan.source.is_empty() {
        plan.source = "ephemeral".into();
    }
    if plan.steps.is_empty() {
        return Err(FinallyAValueBotError::Config("plan has no steps".into()));
    }
    for (i, step) in plan.steps.iter_mut().enumerate() {
        if step.id.is_empty() {
            step.id = (i + 1).to_string();
        }
        if step.allowed_tools.is_empty() {
            step.allowed_tools = default_tool_allowlist();
        }
    }
    Ok(plan)
}

fn fallback_single_step_plan(intent: &IntentDecision) -> Plan {
    Plan {
        source: "ephemeral_fallback".into(),
        vault_path: None,
        steps: vec![PlanStep {
            id: "1".into(),
            goal: intent.restated_goal.clone(),
            allowed_tools: default_tool_allowlist(),
            inputs: String::new(),
            expected_output: "User goal addressed with tool evidence.".into(),
            verification: "Response grounded in tool results.".into(),
        }],
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

pub fn plan_summary(plan: &Plan) -> String {
    let paths = plan.vault_path.as_deref().unwrap_or("(none)");
    format!(
        "source={} vault={} steps={}",
        plan.source,
        paths,
        plan.steps.len()
    )
}
