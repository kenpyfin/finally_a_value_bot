use regex::Regex;
use serde_json::Value;

use crate::db::{Database, HookDefinitionRecord};
use crate::error::FinallyAValueBotError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEventName {
    BeforeTurn,
    PreToolUse,
    PostToolUse,
    PostToolBatch,
    PreStop,
    PostDelivery,
}

impl HookEventName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BeforeTurn => "BeforeTurn",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PostToolBatch => "PostToolBatch",
            Self::PreStop => "PreStop",
            Self::PostDelivery => "PostDelivery",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HookRunInput {
    pub tool_name: Option<String>,
    pub stop_reason: Option<String>,
    pub assistant_text: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct HookRunResult {
    pub blocked_reason: Option<String>,
    pub additional_contexts: Vec<String>,
    pub matched_hook_ids: Vec<i64>,
    /// Matched a `pz_terminal_cleanup` PostToolUse hook; apply via `hook_actions`.
    pub pz_terminal_cleanup: bool,
}

fn event_matches(record: &HookDefinitionRecord, event: HookEventName) -> bool {
    record
        .event_name
        .trim()
        .eq_ignore_ascii_case(event.as_str())
}

fn matcher_target(input: &HookRunInput, event: HookEventName) -> Option<&str> {
    match event {
        HookEventName::PreToolUse | HookEventName::PostToolUse => input.tool_name.as_deref(),
        HookEventName::PreStop | HookEventName::PostDelivery => input.stop_reason.as_deref(),
        _ => None,
    }
}

fn matcher_matches(matcher: Option<&str>, input: &HookRunInput, event: HookEventName) -> bool {
    let Some(matcher) = matcher.map(str::trim).filter(|m| !m.is_empty()) else {
        return true;
    };
    let target = matcher_target(input, event).unwrap_or_default();
    if matcher == "*" {
        return true;
    }
    Regex::new(matcher)
        .map(|re| re.is_match(target))
        .unwrap_or(false)
}

fn parse_payload(payload_json: &str) -> Value {
    serde_json::from_str(payload_json).unwrap_or(Value::Null)
}

fn payload_string(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

pub fn run_hooks_for_event(
    db: &Database,
    chat_id: i64,
    persona_id: i64,
    event: HookEventName,
    input: &HookRunInput,
) -> Result<HookRunResult, FinallyAValueBotError> {
    let hooks = db.list_hook_definitions()?;
    let mut out = HookRunResult::default();
    for hook in hooks {
        if !hook.enabled || !event_matches(&hook, event) {
            continue;
        }
        if !db.is_hook_allowed_for_persona(chat_id, persona_id, hook.id)? {
            continue;
        }
        if !matcher_matches(hook.matcher.as_deref(), input, event) {
            continue;
        }
        out.matched_hook_ids.push(hook.id);
        let payload = parse_payload(&hook.action_payload_json);
        match hook.action_type.trim().to_ascii_lowercase().as_str() {
            "block" => {
                let reason = payload_string(&payload, "reason")
                    .unwrap_or_else(|| format!("Blocked by hook '{}'", hook.name));
                out.blocked_reason = Some(reason);
                break;
            }
            "add_context" => {
                if let Some(text) = payload_string(&payload, "additional_context")
                    .or_else(|| payload_string(&payload, "context"))
                {
                    out.additional_contexts.push(text);
                }
            }
            "pz_terminal_cleanup" => {
                out.pz_terminal_cleanup = true;
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let root = std::env::temp_dir().join(format!(
            "finally_a_value_bot_hook_runtime_test_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Database::new(root.to_str().unwrap()).unwrap()
    }

    #[test]
    fn default_policy_allows_hook() {
        let db = test_db();
        let chat_id = 9001;
        let persona_id = db
            .create_persona(chat_id, "default", None)
            .expect("create persona");
        let hook_id = db
            .upsert_hook_definition(
                None,
                "block-bash",
                HookEventName::PreToolUse.as_str(),
                Some("^bash$"),
                "block",
                r#"{"reason":"blocked"}"#,
                true,
            )
            .expect("upsert hook");
        let out = run_hooks_for_event(
            &db,
            chat_id,
            persona_id,
            HookEventName::PreToolUse,
            &HookRunInput {
                tool_name: Some("bash".to_string()),
                ..HookRunInput::default()
            },
        )
        .expect("run hooks");
        assert_eq!(out.blocked_reason.as_deref(), Some("blocked"));
        assert!(out.matched_hook_ids.contains(&hook_id));
    }

    #[test]
    fn persona_allowlist_blocks_unassigned_hook() {
        let db = test_db();
        let chat_id = 9002;
        let persona_id = db
            .create_persona(chat_id, "default", None)
            .expect("create persona");
        let hook_id = db
            .upsert_hook_definition(
                None,
                "add-note",
                HookEventName::PostToolBatch.as_str(),
                None,
                "add_context",
                r#"{"additional_context":"ctx"}"#,
                true,
            )
            .expect("upsert hook");
        db.set_persona_hook_skill_policy(chat_id, persona_id, Some(&[]), None)
            .expect("set policy");
        let out = run_hooks_for_event(
            &db,
            chat_id,
            persona_id,
            HookEventName::PostToolBatch,
            &HookRunInput::default(),
        )
        .expect("run hooks");
        assert!(out.additional_contexts.is_empty());
        assert!(!out.matched_hook_ids.contains(&hook_id));
    }

    #[test]
    fn pz_terminal_cleanup_action_sets_flag() {
        let db = test_db();
        let chat_id = 9003;
        let persona_id = db
            .create_persona(chat_id, "default", None)
            .expect("create persona");
        db.upsert_hook_definition(
            None,
            "posttool-pz-terminal-cleanup",
            HookEventName::PostToolUse.as_str(),
            None,
            "pz_terminal_cleanup",
            "{}",
            true,
        )
        .expect("upsert hook");
        let out = run_hooks_for_event(
            &db,
            chat_id,
            persona_id,
            HookEventName::PostToolUse,
            &HookRunInput {
                tool_name: Some("bash".to_string()),
                ..HookRunInput::default()
            },
        )
        .expect("run hooks");
        assert!(out.pz_terminal_cleanup);
    }
}
