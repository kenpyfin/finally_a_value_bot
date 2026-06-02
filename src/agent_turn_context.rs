//! Session goal extraction for the current agent turn (`[current_request]`).

use crate::claude::{ContentBlock, Message, MessageContent};
use crate::safety_redaction::EnvSecretRedactor;

/// Primary task for this turn — aligned with task-first context restructuring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionGoalContext {
    pub current_request: String,
    pub is_short_reply: bool,
    pub disambiguation_assistant: Option<String>,
}

const SHORT_REPLY_MAX_CHARS: usize = 40;

/// Parse body from `[current_request]...[/current_request]`.
pub fn parse_current_request_content(text: &str) -> Option<String> {
    let text = text.trim();
    if !text.starts_with("[current_request") {
        return None;
    }
    let body_start = text.find(']')? + 1;
    let body_end = text.rfind("[/current_request]")?;
    if body_end <= body_start {
        return None;
    }
    Some(text[body_start..body_end].trim().to_string())
}

fn text_from_message_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn parse_wrapped_user_message(text: &str) -> Option<(String, Option<String>, String)> {
    let text = text.trim();
    const PREFIX: &str = "<user_message ";
    const SUFFIX: &str = "</user_message>";
    if !text.starts_with(PREFIX) || !text.ends_with(SUFFIX) {
        return None;
    }
    let tag_body = text[PREFIX.len()..text.len() - SUFFIX.len()].trim_start();
    let close = tag_body.find('>')?;
    let attrs = &tag_body[..close];
    let content = tag_body[close + 1..].to_string();
    let sender = extract_xml_attr(attrs, "sender")?;
    let at = extract_xml_attr(attrs, "at");
    Some((sender, at, content))
}

fn extract_xml_attr(attrs: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = attrs.find(&needle)? + needle.len();
    let rest = &attrs[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn latest_user_text_fallback(messages: &[Message]) -> String {
    for m in messages.iter().rev() {
        if m.role != "user" {
            continue;
        }
        let text = text_from_message_content(&m.content);
        if let Some(inner) = parse_current_request_content(&text) {
            return inner;
        }
        if let Some((_, _, inner)) = parse_wrapped_user_message(&text) {
            return inner;
        }
        if let Some(rest) = text.strip_prefix("[scheduler]: ") {
            return rest.to_string();
        }
        if !text.starts_with('[') {
            return text;
        }
    }
    String::new()
}

fn assistant_text_from_message(msg: &Message) -> String {
    if msg.role != "assistant" {
        return String::new();
    }
    text_from_message_content(&msg.content)
}

/// Heuristic: short follow-up that needs prior assistant context to interpret.
pub fn is_short_reply_request(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if t.chars().count() <= SHORT_REPLY_MAX_CHARS {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    const PHRASES: &[&str] = &[
        "yes",
        "no",
        "yep",
        "nope",
        "ok",
        "okay",
        "sure",
        "use the",
        "use option",
        "option a",
        "option b",
        "the first",
        "the second",
        "go ahead",
        "do it",
        "retry",
        "wait",
    ];
    PHRASES
        .iter()
        .any(|p| lower == *p || lower.starts_with(&format!("{p} ")))
}

fn truncate_for_eval(text: &str, max_chars: usize, redactor: Option<&EnvSecretRedactor>) -> String {
    let t = if let Some(r) = redactor {
        r.redact(text)
    } else {
        text.to_string()
    };
    if t.chars().count() <= max_chars {
        t
    } else {
        format!("{}...", t.chars().take(max_chars).collect::<String>())
    }
}

/// Extract the session goal from the message list the agent loop used.
pub fn extract_session_goal(
    messages: &[Message],
    redactor: Option<&EnvSecretRedactor>,
) -> SessionGoalContext {
    let mut current_request = String::new();
    let mut current_request_idx = None;

    for (idx, msg) in messages.iter().enumerate().rev() {
        if msg.role != "user" {
            continue;
        }
        let text = text_from_message_content(&msg.content);
        if let Some(body) = parse_current_request_content(&text) {
            current_request = body;
            current_request_idx = Some(idx);
            break;
        }
    }

    if current_request.is_empty() {
        current_request = latest_user_text_fallback(messages);
    }

    let is_short_reply = is_short_reply_request(&current_request);

    let disambiguation_assistant = if is_short_reply {
        current_request_idx.and_then(|idx| {
            messages[..idx].iter().rev().find_map(|m| {
                let t = assistant_text_from_message(m);
                if t.trim().is_empty() {
                    None
                } else {
                    Some(truncate_for_eval(&t, 800, redactor))
                }
            })
        })
    } else {
        None
    };

    SessionGoalContext {
        current_request: truncate_for_eval(&current_request, 2000, redactor),
        is_short_reply,
        disambiguation_assistant,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_current_request() {
        let t = "[current_request sender=\"alice\"]\nfix the bug\n[/current_request]";
        assert_eq!(
            parse_current_request_content(t).as_deref(),
            Some("fix the bug")
        );
    }

    #[test]
    fn extract_session_goal_from_current_request() {
        let messages = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text(
                    "<user_message context=\"prior_turn\" sender=\"u\">old</user_message>".into(),
                ),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text(
                    "<assistant_message context=\"prior_turn\">pick A or B?</assistant_message>"
                        .into(),
                ),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Text(
                    "[current_request sender=\"u\"]\nB\n[/current_request]".into(),
                ),
            },
        ];
        let goal = extract_session_goal(&messages, None);
        assert_eq!(goal.current_request, "B");
        assert!(goal.is_short_reply);
        assert!(goal
            .disambiguation_assistant
            .as_ref()
            .unwrap()
            .contains("A or B"));
    }

    #[test]
    fn is_short_reply_detects_yes() {
        assert!(is_short_reply_request("yes"));
        assert!(!is_short_reply_request(
            "please regenerate the image with the blue background"
        ));
    }
}
