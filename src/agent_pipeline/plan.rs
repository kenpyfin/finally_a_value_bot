//! Workflow / plan resolution for the deterministic pipeline.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use crate::agent_pipeline::profile::{OperationalConfig, PolicyConfig, ResolvedPhase};
use crate::claude::{Message, MessageContent};
use crate::error::FinallyAValueBotError;
use crate::memory::{PersonaMemoryState, SopPointer};
use crate::telegram::AppState;

use super::cloud_context::PipelineCloudContext;
use super::intent::{
    self, extract_json_object, json_stage_tier, IntentDecision, IntentOutcome,
    INTENT_PLAN_TIMEOUT_SECS,
};
use super::skill_script_contract::{
    format_contract_args_hint, load_skill_script_contract, resolve_default_skill_script,
};

pub const MAX_EPHEMERAL_STEPS: usize = 4;

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
    #[serde(default)]
    pub skill_name: Option<String>,
    #[serde(default)]
    pub skill_script: Option<String>,
    #[serde(default)]
    pub skill_args_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub source: String,
    #[serde(default)]
    pub vault_path: Option<String>,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClassifyAndPlanResponse {
    #[serde(default)]
    intent: Option<IntentDecision>,
    plan: Plan,
}

pub(crate) const PLAN_SYSTEM: &str = "\
You are a task planner for a cloud LLM stage. You receive pipeline_cloud_context (skills catalog, conversation, memory). \
Produce a flexible JSON execution plan for a separate LOCAL executor that will see ONLY each step's fields — no full chat history.\n\
Default plan source is \"ephemeral\" — plan from the user request and cloud context. Do NOT assume a persona SOP applies unless a REFERENCE SOP block is explicitly provided below.\n\
Schema: {\"source\":\"ephemeral\",\"steps\":[{\"id\":\"1\",\"goal\":\"...\",\
\"allowed_tools\":[\"bash\",\"read_file\"],\"inputs\":\"precise local instructions (paths, args, file names)\",\
\"expected_output\":\"...\",\
\"verification\":\"how to verify success\",\
\"skill_name\":null,\"skill_script\":null,\"skill_args_hint\":null}]}\n\
Rules: 1-4 steps default; each step one clear outcome; pick tools that match the request (e.g. glob/read_file for show/find image, search_chat_history for prior chat artifacts, search_vault only for semantic vault lookup). \
allowed_tools from common agent tools \
(bash, read_file, grep, glob, search_vault, search_chat_history, activate_skill, run_skill_script, write_file, edit_file); \
verification is a short pass/fail criterion.\n\
skill_name MUST be an exact id from the skills catalog — NEVER invent skills or script filenames. \
Prefer the search_vault TOOL over run_skill_script unless SKILL.md requires a specific script. \
When using run_skill_script, set skill_name, skill_script (filename from SKILL.md ## Scripts), and skill_args_hint with the script's real CLI shape (positional args or documented flags only).\n\
If a REFERENCE SOP block is provided (rare), use it only for steps required by the CURRENT request — skip unrelated SOP stages. \
When an SOP stage runs a skill, merge activate_skill + run_skill_script into ONE step.\n\
Write inputs/expected_output/skill_args_hint as explicit runbook text the local model must follow verbatim.";

pub fn builtin_plan_system_prompt() -> &'static str {
    PLAN_SYSTEM
}

pub(crate) const CLASSIFY_AND_PLAN_SYSTEM: &str = "\
You classify intent and produce an execution plan in one JSON object (no markdown fences). \
Use pipeline_cloud_context (skills catalog, conversation, memory). \
Local execute receives ONLY step contracts — be explicit in skill_name, skill_script, skill_args_hint, and inputs.\n\
Schema: {\"intent\":{\"category\":\"conversational\"|\"question\"|\"task\",\"restated_goal\":\"...\",\
\"success_criteria\":[],\"needs_clarification\":false,\"clarifying_questions\":[],\
\"candidate_sop_hint\":null,\"assumptions_if_proceeding\":[]},\
\"plan\":{\"source\":\"ephemeral\",\"steps\":[...]}}\n\
Plan flexibly from the user request (default source ephemeral). Set candidate_sop_hint only when a specific ORIGIN/ vault SOP clearly applies. \
Use the same planning rules as a task planner: 1-4 steps default; skill_name must match the catalog exactly; \
prefer search_vault tool over run_skill_script for semantic vault search; merge skill activate+run steps when a script is required.";

/// Maximum characters of an SOP body injected into the planner prompt as reference.
pub(crate) const SOP_REFERENCE_MAX_CHARS: usize = 6000;

/// A vault SOP matched for the current request, passed to the planner as reference context.
pub(crate) struct SopReference {
    id: String,
    vault_path: String,
    body: String,
}

pub async fn resolve_plan_with_config(
    state: &AppState,
    intent: &IntentDecision,
    memory_state: Option<&PersonaMemoryState>,
    user_request: &str,
    cloud_context: Option<&PipelineCloudContext>,
    resolved: Option<&ResolvedPhase<'_>>,
    operational: Option<&OperationalConfig>,
    policies: Option<&PolicyConfig>,
    agent_system_prompt: Option<&str>,
) -> Result<Plan, FinallyAValueBotError> {
    let includes = resolved
        .map(|r| &r.phase.context_includes)
        .cloned()
        .unwrap_or_default();
    let bind_persona_sops = policies
        .map(|p| p.bind_persona_sops_in_plan)
        .unwrap_or(false);
    let sop_ref = if includes.include_sop_reference {
        find_sop_reference(state, Some(intent), memory_state, bind_persona_sops).await?
    } else {
        None
    };
    let mut plan = generate_plan(
        state,
        intent,
        user_request,
        sop_ref.as_ref(),
        cloud_context,
        resolved,
        operational,
        policies,
        agent_system_prompt,
    )
    .await?;
    let max_steps = operational
        .map(|o| o.max_plan_steps)
        .unwrap_or(MAX_EPHEMERAL_STEPS);
    let known_skills = cloud_context.map(|c| &c.known_skill_names);
    normalize_plan(&mut plan, Some(state), max_steps, known_skills);
    Ok(plan)
}

/// Single LLM call returning intent + plan for task paths. Returns `None` on parse failure.
pub async fn try_classify_and_plan_with_config(
    state: &AppState,
    user_request: &str,
    memory_state: Option<&PersonaMemoryState>,
    intent_hint: Option<&IntentDecision>,
    cloud_context: Option<&PipelineCloudContext>,
    resolved: Option<&ResolvedPhase<'_>>,
    operational: Option<&OperationalConfig>,
    policies: Option<&PolicyConfig>,
    agent_system_prompt: Option<&str>,
) -> Result<Option<(IntentOutcome, Plan)>, FinallyAValueBotError> {
    let includes = resolved
        .map(|r| &r.phase.context_includes)
        .cloned()
        .unwrap_or_default();
    let bind_persona_sops = policies
        .map(|p| p.bind_persona_sops_in_plan)
        .unwrap_or(false);
    let sop_ref = if includes.include_sop_reference {
        find_sop_reference(state, intent_hint, memory_state, bind_persona_sops).await?
    } else {
        None
    };
    let tier = if let Some(r) = resolved {
        crate::agent_pipeline::profile::resolve_model_tier(r.phase.model_route, state, &r.policies)
    } else {
        json_stage_tier(state, policies)
    };
    let system = if let Some(r) = resolved {
        crate::agent_pipeline::profile::compose_system_prompt(
            r.phase,
            r.phase.kind,
            agent_system_prompt,
            &includes,
        )
    } else {
        CLASSIFY_AND_PLAN_SYSTEM.to_string()
    };
    let timeout_secs = operational
        .map(|o| o.timeout_secs)
        .unwrap_or(INTENT_PLAN_TIMEOUT_SECS);
    let sop_max = operational
        .map(|o| o.sop_reference_max_chars)
        .unwrap_or(SOP_REFERENCE_MAX_CHARS);
    let sop_block = if includes.include_sop_reference {
        sop_reference_block(sop_ref.as_ref(), sop_max)
    } else {
        String::new()
    };
    let hint_block = intent_hint
        .map(|i| {
            format!(
                "\nHeuristic intent hint: category={:?} goal={}",
                i.category, i.restated_goal
            )
        })
        .unwrap_or_default();
    let request_block = if includes.include_current_request {
        format!(
            "User request:\n{request}",
            request = user_request.chars().take(4000).collect::<String>()
        )
    } else {
        String::new()
    };
    let user_body = format!(
        "{request}{hint}{sop}\n\nRespond with combined JSON only.",
        request = if request_block.is_empty() {
            String::new()
        } else {
            format!("{request_block}\n")
        },
        hint = hint_block,
        sop = sop_block,
    );
    let user_msg = cloud_context
        .map(|c| c.append_to_user_message(&user_body, &includes))
        .unwrap_or(user_body);
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user_msg),
    }];
    let response = timeout(
        Duration::from_secs(timeout_secs),
        state
            .llm
            .send_message_for_tier(tier, &system, messages, None),
    )
    .await
    .map_err(|_| FinallyAValueBotError::LlmApi("classify_and_plan timeout".into()))??;

    let text = intent::response_text(&response.content);
    let max_steps = operational
        .map(|o| o.max_plan_steps)
        .unwrap_or(MAX_EPHEMERAL_STEPS);
    let parsed = parse_classify_and_plan_json(
        &text,
        intent_hint,
        sop_ref.as_ref(),
        Some(state),
        max_steps,
        cloud_context.map(|c| &c.known_skill_names),
    );
    Ok(parsed)
}

async fn find_sop_reference(
    state: &AppState,
    intent: Option<&IntentDecision>,
    memory_state: Option<&PersonaMemoryState>,
    bind_persona_sops: bool,
) -> Result<Option<SopReference>, FinallyAValueBotError> {
    let candidates = collect_sop_candidates(intent, memory_state, bind_persona_sops);
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
    intent: Option<&IntentDecision>,
    memory_state: Option<&PersonaMemoryState>,
    bind_persona_sops: bool,
) -> Vec<SopPointer> {
    let mut out: Vec<SopPointer> = Vec::new();
    if let Some(decision) = intent {
        if let Some(hint) = decision.candidate_sop_hint.as_deref() {
            let hint = hint.trim();
            if hint.starts_with("ORIGIN/") {
                out.push(SopPointer {
                    id: "hint".into(),
                    vault_path: hint.into(),
                    summary: String::new(),
                });
            } else if let Some(ms) = memory_state {
                if let Some(sop) = ms.tier2.sops.iter().find(|s| s.id == hint) {
                    out.push(sop.clone());
                }
            }
        }
    }
    if bind_persona_sops {
        if let Some(ms) = memory_state {
            for sop in &ms.tier2.sops {
                if !out
                    .iter()
                    .any(|existing| existing.vault_path == sop.vault_path)
                {
                    out.push(sop.clone());
                }
            }
        }
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

pub fn default_tool_allowlist() -> Vec<String> {
    vec![
        "bash".into(),
        "read_file".into(),
        "grep".into(),
        "glob".into(),
        "search_vault".into(),
        "search_chat_history".into(),
        "activate_skill".into(),
        "run_skill_script".into(),
        "write_file".into(),
        "edit_file".into(),
        "web_search".into(),
        "web_fetch".into(),
    ]
}

fn sop_reference_block(sop_ref: Option<&SopReference>, max_chars: usize) -> String {
    match sop_ref {
        Some(sop) => format!(
            "\n\nREFERENCE SOP ({path}):\n{body}\n\
             Use the SOP for canonical procedures, but plan ONLY the steps required for the \
             current request; skip unrelated SOP stages.",
            path = sop.vault_path,
            body = truncate(&sop.body, max_chars),
        ),
        None => String::new(),
    }
}

async fn generate_plan(
    state: &AppState,
    intent: &IntentDecision,
    user_request: &str,
    sop_ref: Option<&SopReference>,
    cloud_context: Option<&PipelineCloudContext>,
    resolved: Option<&ResolvedPhase<'_>>,
    operational: Option<&OperationalConfig>,
    policies: Option<&PolicyConfig>,
    agent_system_prompt: Option<&str>,
) -> Result<Plan, FinallyAValueBotError> {
    let includes = resolved
        .map(|r| &r.phase.context_includes)
        .cloned()
        .unwrap_or_default();
    let criteria = if intent.success_criteria.is_empty() {
        "Infer reasonable success criteria.".into()
    } else {
        intent.success_criteria.join("; ")
    };
    let sop_max = operational
        .map(|o| o.sop_reference_max_chars)
        .unwrap_or(SOP_REFERENCE_MAX_CHARS);
    let sop_block = if includes.include_sop_reference {
        sop_reference_block(sop_ref, sop_max)
    } else {
        String::new()
    };
    let request_line = if includes.include_current_request {
        format!(
            "User request:\n{}\n",
            user_request.chars().take(4000).collect::<String>()
        )
    } else {
        String::new()
    };
    let user_body = format!(
        "Goal: {}\nSuccess criteria: {}{}{}\n\nRespond with JSON plan only.",
        intent.restated_goal, criteria, request_line, sop_block,
    );
    let user_msg = cloud_context
        .map(|c| c.append_to_user_message(&user_body, &includes))
        .unwrap_or(user_body);
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user_msg),
    }];
    let tier = if let Some(r) = resolved {
        crate::agent_pipeline::profile::resolve_model_tier(r.phase.model_route, state, &r.policies)
    } else {
        json_stage_tier(state, policies)
    };
    let system = if let Some(r) = resolved {
        crate::agent_pipeline::profile::compose_system_prompt(
            r.phase,
            r.phase.kind,
            agent_system_prompt,
            &includes,
        )
    } else {
        PLAN_SYSTEM.to_string()
    };
    let timeout_secs = operational
        .map(|o| o.timeout_secs)
        .unwrap_or(INTENT_PLAN_TIMEOUT_SECS);
    let response = timeout(
        Duration::from_secs(timeout_secs),
        state
            .llm
            .send_message_for_tier(tier, &system, messages, None),
    )
    .await
    .map_err(|_| FinallyAValueBotError::LlmApi("plan LLM timeout".into()))??;
    let text = intent::response_text(&response.content);
    let mut plan = parse_plan_json(&text).unwrap_or_else(|_| fallback_single_step_plan(intent));
    if let Some(sop) = sop_ref {
        plan.source = format!("vault_sop_scoped:{}", sop.id);
        plan.vault_path = Some(sop.vault_path.clone());
    }
    Ok(plan)
}

fn parse_classify_and_plan_json(
    text: &str,
    intent_hint: Option<&IntentDecision>,
    sop_ref: Option<&SopReference>,
    state: Option<&AppState>,
    max_plan_steps: usize,
    known_skills: Option<&std::collections::HashSet<String>>,
) -> Option<(IntentOutcome, Plan)> {
    let json_str = extract_json_object(text.trim());
    let mut parsed: ClassifyAndPlanResponse = serde_json::from_str(json_str).ok()?;
    let mut intent = parsed.intent.take().or_else(|| intent_hint.cloned())?;
    if intent.restated_goal.trim().is_empty() {
        intent.restated_goal = intent_hint
            .map(|h| h.restated_goal.clone())
            .unwrap_or_else(|| "Complete the user's request.".into());
    }
    let mut plan = parsed.plan;
    if plan.source.is_empty() {
        plan.source = "ephemeral".into();
    }
    if let Some(sop) = sop_ref {
        plan.source = format!("vault_sop_scoped:{}", sop.id);
        plan.vault_path = Some(sop.vault_path.clone());
    }
    normalize_plan(&mut plan, state, max_plan_steps, known_skills);
    if plan.steps.is_empty() {
        return None;
    }
    Some((
        IntentOutcome {
            decision: intent,
            heuristic: false,
            cloud_calls: 1,
        },
        plan,
    ))
}

fn parse_plan_json(text: &str) -> Result<Plan, FinallyAValueBotError> {
    let json_str = extract_json_object(text.trim());
    let mut plan: Plan = serde_json::from_str(json_str)
        .map_err(|e| FinallyAValueBotError::Config(format!("plan JSON parse: {e}")))?;
    if plan.source.is_empty() {
        plan.source = "ephemeral".into();
    }
    if plan.steps.is_empty() {
        return Err(FinallyAValueBotError::Config("plan has no steps".into()));
    }
    normalize_plan(&mut plan, None, MAX_EPHEMERAL_STEPS, None);
    Ok(plan)
}

pub fn normalize_plan(
    plan: &mut Plan,
    state: Option<&AppState>,
    max_plan_steps: usize,
    known_skills: Option<&std::collections::HashSet<String>>,
) {
    for (i, step) in plan.steps.iter_mut().enumerate() {
        if step.id.is_empty() {
            step.id = (i + 1).to_string();
        }
        if step.allowed_tools.is_empty() {
            step.allowed_tools = default_tool_allowlist();
        }
        ensure_skill_tools(step);
        apply_skill_contract_defaults(step, state);
    }
    validate_plan_skill_names(plan, known_skills);
    clamp_plan_steps(plan, max_plan_steps);
}

fn validate_plan_skill_names(
    plan: &mut Plan,
    known_skills: Option<&std::collections::HashSet<String>>,
) {
    let Some(known) = known_skills else {
        return;
    };
    if known.is_empty() {
        return;
    }
    for step in &mut plan.steps {
        let Some(name) = step.skill_name.clone() else {
            continue;
        };
        if known.contains(&name) {
            continue;
        }
        tracing::warn!(
            "plan step {} references unknown skill '{name}' — stripping (use catalog ids only)",
            step.id
        );
        step.inputs = format!(
            "{}{}[planner_skill_rejected] Unknown skill '{name}'. Use file tools or a catalog skill id only.\n",
            step.inputs,
            if step.inputs.is_empty() || step.inputs.ends_with('\n') {
                ""
            } else {
                "\n"
            }
        );
        step.skill_name = None;
        step.skill_script = None;
        step.skill_args_hint = None;
    }
}

fn apply_skill_contract_defaults(step: &mut PlanStep, state: Option<&AppState>) {
    if step.skill_name.is_none() && step.skill_script.is_none() {
        return;
    }
    let Some(state) = state else {
        return;
    };
    if let Some(skill_name) = step.skill_name.clone() {
        if step.skill_script.is_none() {
            if let Some(script) = resolve_default_skill_script(state, &skill_name) {
                step.skill_script = Some(script);
            }
        }
        if step.skill_args_hint.is_none() {
            if let Some(script) = step.skill_script.as_deref() {
                if let Some(contract) = load_skill_script_contract(state, &skill_name, script) {
                    step.skill_args_hint = Some(format_contract_args_hint(&contract));
                }
            }
        }
    } else if let Some(script) = step.skill_script.clone() {
        // skill_script without skill_name — cannot load contract without skill id
        if step.skill_args_hint.is_none() {
            step.skill_args_hint = Some(format!(
                "run_skill_script(skill_name=\"<skill>\", script=\"{script}\", args=[...])"
            ));
        }
    }
}

fn ensure_skill_tools(step: &mut PlanStep) {
    let needs_skill = step.skill_name.is_some()
        || step.skill_script.is_some()
        || step.inputs.contains("run_skill_script")
        || step.inputs.contains("activate_skill");
    if !needs_skill {
        return;
    }
    for tool in ["activate_skill", "run_skill_script"] {
        if !step.allowed_tools.iter().any(|t| t == tool) {
            step.allowed_tools.push(tool.into());
        }
    }
}

pub fn clamp_plan_steps(plan: &mut Plan, max_plan_steps: usize) {
    if plan.steps.len() <= max_plan_steps {
        return;
    }
    let overflow: Vec<_> = plan.steps.drain(max_plan_steps..).map(|s| s.goal).collect();
    if let Some(last) = plan.steps.last_mut() {
        last.goal = format!("{} (also covers: {})", last.goal, overflow.join("; "));
    }
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
            skill_name: None,
            skill_script: None,
            skill_args_hint: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_merges_overflow_into_last_step() {
        let mut plan = Plan {
            source: "ephemeral".into(),
            vault_path: None,
            steps: (1..=6)
                .map(|i| PlanStep {
                    id: i.to_string(),
                    goal: format!("goal {i}"),
                    allowed_tools: vec![],
                    inputs: String::new(),
                    expected_output: String::new(),
                    verification: String::new(),
                    skill_name: None,
                    skill_script: None,
                    skill_args_hint: None,
                })
                .collect(),
        };
        normalize_plan(&mut plan, None, MAX_EPHEMERAL_STEPS, None);
        assert_eq!(plan.steps.len(), MAX_EPHEMERAL_STEPS);
        assert!(plan.steps.last().unwrap().goal.contains("also covers"));
    }

    #[test]
    fn rejects_unknown_skill_name_in_plan() {
        let mut known = std::collections::HashSet::new();
        known.insert("resume-pdf".into());
        let mut plan = Plan {
            source: "ephemeral".into(),
            vault_path: None,
            steps: vec![PlanStep {
                id: "1".into(),
                goal: "Generate PDF".into(),
                allowed_tools: vec![],
                inputs: String::new(),
                expected_output: String::new(),
                verification: String::new(),
                skill_name: Some("write_professional_summary".into()),
                skill_script: Some("write_professional_summary.py".into()),
                skill_args_hint: None,
            }],
        };
        normalize_plan(&mut plan, None, MAX_EPHEMERAL_STEPS, Some(&known));
        assert!(plan.steps[0].skill_name.is_none());
        assert!(plan.steps[0].inputs.contains("planner_skill_rejected"));
    }

    #[test]
    fn collect_sop_candidates_skips_persona_sops_by_default() {
        use super::super::intent::{IntentCategory, IntentDecision};
        use crate::memory::{PersonaMemoryState, SopPointer};
        let memory = PersonaMemoryState {
            tier2: crate::memory::Tier2Memory {
                sops: vec![SopPointer {
                    id: "PZ-Post-Pipeline".into(),
                    vault_path: "ORIGIN/Operations/SOPs/PZ-Post-Pipeline.md".into(),
                    summary: String::new(),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let intent = IntentDecision {
            category: IntentCategory::Question,
            restated_goal: "show image".into(),
            success_criteria: vec![],
            needs_clarification: false,
            clarifying_questions: vec![],
            candidate_sop_hint: None,
            assumptions_if_proceeding: vec![],
        };
        assert!(collect_sop_candidates(Some(&intent), Some(&memory), false).is_empty());
        assert_eq!(
            collect_sop_candidates(Some(&intent), Some(&memory), true).len(),
            1
        );
    }
}
