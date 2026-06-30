//! Structured intent classification for the deterministic pipeline.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use super::cloud_context::PipelineCloudContext;
use crate::agent_pipeline::profile::{OperationalConfig, PolicyConfig, ResolvedPhase};
use crate::claude::{Message, MessageContent, ResponseContentBlock};
use crate::error::FinallyAValueBotError;
use crate::multimodel::ModelTier;
use crate::telegram::AppState;

pub const INTENT_PLAN_TIMEOUT_SECS: u64 = 45;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentCategory {
    Conversational,
    Question,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentDecision {
    pub category: IntentCategory,
    pub restated_goal: String,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub needs_clarification: bool,
    #[serde(default)]
    pub clarifying_questions: Vec<String>,
    #[serde(default)]
    pub candidate_sop_hint: Option<String>,
    #[serde(default)]
    pub assumptions_if_proceeding: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IntentOutcome {
    pub decision: IntentDecision,
    pub heuristic: bool,
    pub cloud_calls: u32,
}

pub(crate) const INTENT_SYSTEM: &str = "\
You are an intent classifier for an agentic assistant. \
Analyze the user's current request using the pipeline_cloud_context block (skills catalog, conversation, memory). \
Respond with JSON only (no markdown fences). \
Schema:\n\
{\"category\":\"conversational\"|\"question\"|\"task\",\
\"restated_goal\":\"one sentence\",\
\"success_criteria\":[\"measurable outcomes\"],\
\"needs_clarification\":true|false,\
\"clarifying_questions\":[\"only when needs_clarification\"],\
\"candidate_sop_hint\":\"optional ORIGIN/ vault path or Tier 2 SOP id when a vault procedure applies\",\
\"assumptions_if_proceeding\":[\"when ambiguous but executable\"]}\n\
Rules: conversational = greetings/small talk; question = factual lookup without tool actions; \
task = requires tools, files, scheduling, or multi-step work. \
Set needs_clarification only when the request cannot be executed without missing critical facts. \
For tasks that will use skills, note the exact skill id from the catalog in success_criteria when relevant. \
Set candidate_sop_hint ONLY when a specific ORIGIN/ vault SOP clearly governs the procedure (not for display/lookup/chat-history requests).";

pub fn builtin_intent_system_prompt() -> &'static str {
    INTENT_SYSTEM
}

const CONVERSATIONAL_PHRASES: &[&str] = &[
    "hi",
    "hello",
    "hey",
    "thanks",
    "thank you",
    "ok",
    "okay",
    "good morning",
    "good night",
    "bye",
    "goodbye",
];

const TASK_ACTION_MARKERS: &[&str] = &[
    "grep",
    "read",
    "run",
    "fix",
    "deploy",
    "edit",
    "write",
    "search",
    "create",
    "update",
    "delete",
    "install",
    "build",
    "execute",
    "hotify",
    "schedule",
    "activate",
    "sync",
    "upload",
    "download",
    "commit",
    "push",
    "pull",
    "merge",
    "refactor",
    "implement",
];

/// Fast path: return `Some` when intent is confident without an LLM call.
pub fn classify_intent_fast(user_request: &str) -> Option<IntentDecision> {
    let trimmed = user_request.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    let normalized = lower.trim();

    if CONVERSATIONAL_PHRASES
        .iter()
        .any(|p| normalized == *p || normalized.starts_with(&format!("{p} ")))
    {
        return Some(heuristic_decision(
            IntentCategory::Conversational,
            user_request,
        ));
    }

    if looks_like_question(&lower) && !looks_like_clear_task(&lower) {
        return Some(heuristic_decision(IntentCategory::Question, user_request));
    }

    if looks_like_clear_task(&lower) {
        return Some(IntentDecision {
            category: IntentCategory::Task,
            restated_goal: user_request.chars().take(500).collect(),
            success_criteria: vec![],
            needs_clarification: false,
            clarifying_questions: vec![],
            candidate_sop_hint: None,
            assumptions_if_proceeding: vec![],
        });
    }

    None
}

fn looks_like_question(lower: &str) -> bool {
    lower.ends_with('?')
        || lower.starts_with("what ")
        || lower.starts_with("how ")
        || lower.starts_with("why ")
        || lower.starts_with("when ")
        || lower.starts_with("where ")
        || lower.starts_with("who ")
        || lower.starts_with("is ")
        || lower.starts_with("are ")
        || lower.starts_with("can ")
}

fn looks_like_clear_task(lower: &str) -> bool {
    TASK_ACTION_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

fn heuristic_decision(category: IntentCategory, text: &str) -> IntentDecision {
    IntentDecision {
        category,
        restated_goal: text.chars().take(500).collect(),
        success_criteria: vec![],
        needs_clarification: false,
        clarifying_questions: vec![],
        candidate_sop_hint: None,
        assumptions_if_proceeding: vec![],
    }
}

pub async fn classify_intent_with_config(
    state: &AppState,
    user_request: &str,
    has_image: bool,
    cloud_context: Option<&PipelineCloudContext>,
    resolved: Option<&ResolvedPhase<'_>>,
    operational: Option<&OperationalConfig>,
    policies: Option<&PolicyConfig>,
    agent_system_prompt: Option<&str>,
) -> Result<IntentOutcome, FinallyAValueBotError> {
    let default_policies = PolicyConfig::default();
    let policies = policies.unwrap_or(&default_policies);
    if has_image && policies.image_input_force_task {
        return Ok(IntentOutcome {
            decision: IntentDecision {
                category: IntentCategory::Task,
                restated_goal: user_request.chars().take(500).collect(),
                success_criteria: vec!["Process the attached image per the user request.".into()],
                needs_clarification: false,
                clarifying_questions: vec![],
                candidate_sop_hint: None,
                assumptions_if_proceeding: vec![],
            },
            heuristic: true,
            cloud_calls: 0,
        });
    }

    if policies.heuristic_intent_enabled {
        if let Some(fast) = classify_intent_fast(user_request) {
            return Ok(IntentOutcome {
                decision: fast,
                heuristic: true,
                cloud_calls: 0,
            });
        }
    }

    let decision = classify_intent_llm(
        state,
        user_request,
        cloud_context,
        resolved,
        operational,
        Some(policies),
        agent_system_prompt,
    )
    .await?;
    Ok(IntentOutcome {
        decision,
        heuristic: false,
        cloud_calls: 1,
    })
}

async fn classify_intent_llm(
    state: &AppState,
    user_request: &str,
    cloud_context: Option<&PipelineCloudContext>,
    resolved: Option<&ResolvedPhase<'_>>,
    operational: Option<&OperationalConfig>,
    policies: Option<&PolicyConfig>,
    agent_system_prompt: Option<&str>,
) -> Result<IntentDecision, FinallyAValueBotError> {
    let includes = resolved
        .map(|r| &r.phase.context_includes)
        .cloned()
        .unwrap_or_default();
    let user_body = if includes.include_current_request {
        format!(
            "Current user request:\n{}\n\nRespond with JSON only.",
            user_request.chars().take(4000).collect::<String>()
        )
    } else {
        "Respond with JSON only.".into()
    };
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
        INTENT_SYSTEM.to_string()
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
    .map_err(|_| FinallyAValueBotError::LlmApi("intent LLM timeout".into()))??;

    let text = response_text(&response.content);
    parse_intent_json(&text).or_else(|_| Ok(heuristic_decision(IntentCategory::Task, user_request)))
}

pub(crate) fn json_stage_tier(state: &AppState, policies: Option<&PolicyConfig>) -> ModelTier {
    let default_policies = PolicyConfig::default();
    let policies = policies.unwrap_or(&default_policies);
    crate::agent_pipeline::profile::resolve_model_tier(
        crate::agent_pipeline::profile::ModelRoute::InheritGlobal,
        state,
        policies,
    )
}

pub(crate) fn response_text(blocks: &[ResponseContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ResponseContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn parse_intent_json(text: &str) -> Result<IntentDecision, FinallyAValueBotError> {
    let trimmed = text.trim();
    let json_str = extract_json_object(trimmed);
    let mut decision: IntentDecision = serde_json::from_str(json_str)
        .map_err(|e| FinallyAValueBotError::Config(format!("intent JSON parse: {e}")))?;
    if decision.restated_goal.trim().is_empty() {
        decision.restated_goal = "Complete the user's request.".into();
    }
    Ok(decision)
}

pub(crate) fn extract_json_object(text: &str) -> &str {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return &text[start..=end];
        }
    }
    text
}

pub fn format_clarification_message(decision: &IntentDecision) -> String {
    if decision.clarifying_questions.is_empty() {
        "I need a bit more detail before I can proceed. What outcome are you looking for?"
            .to_string()
    } else {
        decision.clarifying_questions.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_path_greeting() {
        let d = classify_intent_fast("hello").expect("greeting");
        assert_eq!(d.category, IntentCategory::Conversational);
    }

    #[test]
    fn fast_path_question() {
        let d = classify_intent_fast("What is the vault path?").expect("question");
        assert_eq!(d.category, IntentCategory::Question);
    }

    #[test]
    fn fast_path_task_with_action_verb() {
        let d = classify_intent_fast("grep for TODO in src/").expect("task");
        assert_eq!(d.category, IntentCategory::Task);
    }

    #[test]
    fn fast_path_ambiguous_returns_none() {
        assert!(classify_intent_fast("maybe we should think about it").is_none());
    }
}
