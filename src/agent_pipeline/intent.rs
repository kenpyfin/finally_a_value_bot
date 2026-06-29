//! Structured intent classification for the deterministic pipeline.

use serde::{Deserialize, Serialize};

use crate::claude::{Message, MessageContent, ResponseContentBlock};
use crate::error::FinallyAValueBotError;
use crate::multimodel::ModelTier;
use crate::telegram::AppState;

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

const INTENT_SYSTEM: &str = "\
You are an intent classifier for an agentic assistant. \
Analyze the user's current request and respond with JSON only (no markdown fences). \
Schema:\n\
{\"category\":\"conversational\"|\"question\"|\"task\",\
\"restated_goal\":\"one sentence\",\
\"success_criteria\":[\"measurable outcomes\"],\
\"needs_clarification\":true|false,\
\"clarifying_questions\":[\"only when needs_clarification\"],\
\"candidate_sop_hint\":\"optional ORIGIN/ vault path or SOP id if procedure likely applies\",\
\"assumptions_if_proceeding\":[\"when ambiguous but executable\"]}\n\
Rules: conversational = greetings/small talk; question = factual lookup without tool actions; \
task = requires tools, files, scheduling, or multi-step work. \
Set needs_clarification only when the request cannot be executed without missing critical facts.";

pub async fn classify_intent(
    state: &AppState,
    user_request: &str,
    has_image: bool,
) -> Result<IntentDecision, FinallyAValueBotError> {
    if has_image {
        return Ok(IntentDecision {
            category: IntentCategory::Task,
            restated_goal: user_request.chars().take(500).collect(),
            success_criteria: vec!["Process the attached image per the user request.".into()],
            needs_clarification: false,
            clarifying_questions: vec![],
            candidate_sop_hint: None,
            assumptions_if_proceeding: vec![],
        });
    }

    let user_msg = format!(
        "Current user request:\n{}\n\nRespond with JSON only.",
        user_request.chars().take(4000).collect::<String>()
    );
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user_msg),
    }];

    let response = state
        .llm
        .send_message_for_tier(ModelTier::Strategy, INTENT_SYSTEM, messages, None)
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

    parse_intent_json(&text).or_else(|_| Ok(heuristic_intent(user_request)))
}

fn parse_intent_json(text: &str) -> Result<IntentDecision, FinallyAValueBotError> {
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
    let mut decision: IntentDecision = serde_json::from_str(json_str)
        .map_err(|e| FinallyAValueBotError::Config(format!("intent JSON parse: {e}")))?;
    if decision.restated_goal.trim().is_empty() {
        decision.restated_goal = "Complete the user's request.".into();
    }
    Ok(decision)
}

fn heuristic_intent(text: &str) -> IntentDecision {
    let lower = text.to_lowercase();
    let category = if matches!(
        lower.trim(),
        "hi" | "hello" | "hey" | "thanks" | "thank you" | "ok" | "okay"
    ) {
        IntentCategory::Conversational
    } else if lower.contains('?')
        || lower.starts_with("what ")
        || lower.starts_with("how ")
        || lower.starts_with("why ")
        || lower.starts_with("when ")
    {
        IntentCategory::Question
    } else {
        IntentCategory::Task
    };
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

pub fn format_clarification_message(decision: &IntentDecision) -> String {
    if decision.clarifying_questions.is_empty() {
        "I need a bit more detail before I can proceed. What outcome are you looking for?"
            .to_string()
    } else {
        decision.clarifying_questions.join("\n")
    }
}
