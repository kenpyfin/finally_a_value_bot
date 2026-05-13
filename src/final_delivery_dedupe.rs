//! Heuristics to avoid delivering a near-duplicate final assistant reply when
//! `send_message` already posted the substantive text (especially with attachments).

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::db::StoredMessage;

/// After persona normalization, what to actually deliver to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentFinalDeliveryPlan {
    /// Deliver the full proposed final (default).
    DeliverFull,
    /// Main body duplicates a recent `send_message`; deliver only this suffix (raw text, no persona prefix).
    DeliverSuffixOnly(String),
    /// Nothing new to show (duplicate body, no meaningful tail).
    Skip,
}

/// Minimum whitespace-separated tokens on the prior `send_message` body (after stripping
/// attachment footers) before we allow suppressing a final — avoids skipping after a tiny ping.
pub const MIN_PRIOR_WORDS: usize = 35;

/// Sørensen–Dice similarity threshold between prior caption and the main part of the final.
pub const SIMILARITY_THRESHOLD: f64 = 0.84;

/// Strip leading `[Tag] ` transport prefixes (same idea as `channel::strip_leading_persona_tokens`).
fn strip_leading_persona_tokens(text: &str) -> &str {
    let mut rest = text.trim_start();
    loop {
        if !rest.starts_with('[') {
            break;
        }
        let Some(close_idx) = rest.find(']') else {
            break;
        };
        let token = &rest[1..close_idx];
        if token.is_empty() || token.len() > 64 || token.contains('\n') {
            break;
        }
        rest = rest[close_idx + 1..].trim_start();
    }
    rest
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tokenize_words(s: &str) -> Vec<String> {
    strip_leading_persona_tokens(s)
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Multiset Sørensen–Dice on whitespace tokens (bag overlap / average token counts).
fn word_bag_dice(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut ca: HashMap<&str, i32> = HashMap::new();
    for t in a {
        *ca.entry(t.as_str()).or_insert(0) += 1;
    }
    let mut cb: HashMap<&str, i32> = HashMap::new();
    for t in b {
        *cb.entry(t.as_str()).or_insert(0) += 1;
    }
    let mut overlap = 0i64;
    for (k, va) in &ca {
        if let Some(vb) = cb.get(*k) {
            overlap += (*va as i64).min(*vb as i64);
        }
    }
    let sum_a: i64 = ca.values().map(|&v| v as i64).sum();
    let sum_b: i64 = cb.values().map(|&v| v as i64).sum();
    if sum_a + sum_b == 0 {
        return 0.0;
    }
    (2 * overlap) as f64 / (sum_a + sum_b) as f64
}

/// Strip trailing web/Telegram attachment echo from a stored `send_message` row.
pub fn strip_send_message_echo(stored: &str) -> String {
    let base = strip_leading_persona_tokens(stored).trim();
    if let Some(rest) = base.strip_prefix("[attachment:") {
        if let Some(idx) = rest.find(']') {
            return rest[idx + 1..].trim().to_string();
        }
    }
    // Web: caption then blank line then ![alt](url) or [file](url)
    if let Some(pos) = base.rfind("\n\n[") {
        let tail = &base[pos + 2..];
        if tail.contains("](")
            && (tail.contains("/api/uploads/")
                || tail.contains("/upload/")
                || tail.contains("upload/web/"))
        {
            return base[..pos].trim().to_string();
        }
    }
    base.to_string()
}

fn looks_like_send_message_artifact(stored: &str) -> bool {
    let t = strip_leading_persona_tokens(stored);
    if t.trim_start().starts_with("[attachment:") {
        return true;
    }
    t.contains("/api/uploads/")
        || t.contains("/upload/")
        || (t.contains("](") && t.contains("upload/web/"))
}

/// Newest-first scan of `recent_oldest_first` for a bot `send_message` row still inside the window.
pub fn find_send_message_dedupe_anchor(
    recent_oldest_first: &[StoredMessage],
    now: DateTime<Utc>,
    window_secs: i64,
) -> Option<&StoredMessage> {
    recent_oldest_first.iter().rev().find(|m| {
        m.is_from_bot
            && looks_like_send_message_artifact(&m.content)
            && message_age_within_window(&m.timestamp, now, window_secs)
    })
}

/// Split `(main, memory_tail)` when the final ends with a horizontal rule and Memory heading.
pub fn split_memory_tail(proposed_body: &str) -> (&str, Option<&str>) {
    let trimmed = proposed_body.trim_end();
    let idx_len = trimmed
        .rfind("\n\n---\n")
        .map(|i| (i, "\n\n---\n".len()))
        .or_else(|| trimmed.rfind("\n---\n").map(|i| (i, "\n---\n".len())));
    let Some((idx, needle_len)) = idx_len else {
        return (trimmed, None);
    };
    let tail = trimmed[idx + needle_len..].trim_start();
    let head = trimmed[..idx].trim_end();
    let tl = tail.to_ascii_lowercase();
    if tl.starts_with("**memory") || tl.starts_with("memory update") || tl.starts_with("# memory") {
        if head.is_empty() {
            return ("", Some(tail));
        }
        return (head, Some(tail));
    }
    (trimmed, None)
}

pub fn word_similarity_for_dedupe(a: &str, b: &str) -> f64 {
    let ta = tokenize_words(&collapse_ws(a));
    let tb = tokenize_words(&collapse_ws(b));
    word_bag_dice(&ta, &tb)
}

fn message_age_within_window(ts_rfc3339: &str, now: DateTime<Utc>, window_secs: i64) -> bool {
    let Ok(parsed) = DateTime::parse_from_rfc3339(ts_rfc3339) else {
        return false;
    };
    let parsed = parsed.with_timezone(&Utc);
    let age = now.signed_duration_since(parsed);
    age.num_seconds() >= 0 && age.num_seconds() <= window_secs
}

/// Decide how to deliver `proposed_final_indicated` given a recent `send_message` anchor (if any).
/// Call [`find_send_message_dedupe_anchor`] first — the latest bot row is often a memory/tool echo, not `send_message`.
pub fn plan_agent_final_delivery(
    send_message_anchor: Option<&StoredMessage>,
    proposed_final_indicated: &str,
) -> AgentFinalDeliveryPlan {
    let Some(last) = send_message_anchor else {
        return AgentFinalDeliveryPlan::DeliverFull;
    };

    let prior_caption = strip_send_message_echo(&last.content);
    let prior_words = tokenize_words(&prior_caption);
    if prior_words.len() < MIN_PRIOR_WORDS {
        return AgentFinalDeliveryPlan::DeliverFull;
    }

    let proposed_body = strip_leading_persona_tokens(proposed_final_indicated).trim();
    let (main, mem_tail) = split_memory_tail(proposed_body);

    if main.is_empty() && mem_tail.is_none() {
        return AgentFinalDeliveryPlan::Skip;
    }

    let main_for_sim = strip_send_message_echo(main);
    let sim = word_similarity_for_dedupe(&prior_caption, &main_for_sim);
    if sim < SIMILARITY_THRESHOLD {
        return AgentFinalDeliveryPlan::DeliverFull;
    }

    // High overlap with prior send_message body.
    if let Some(tail) = mem_tail {
        let t = tail.trim();
        if t.is_empty() {
            AgentFinalDeliveryPlan::Skip
        } else {
            AgentFinalDeliveryPlan::DeliverSuffixOnly(t.to_string())
        }
    } else if collapse_ws(proposed_final_indicated) == collapse_ws(&last.content) {
        AgentFinalDeliveryPlan::Skip
    } else {
        // Paraphrase without a separable memory tail — still duplicate for the user.
        AgentFinalDeliveryPlan::Skip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(content: &str) -> StoredMessage {
        StoredMessage {
            id: "1".into(),
            chat_id: 1,
            persona_id: 3,
            sender_name: "bot".into(),
            content: content.into(),
            is_from_bot: true,
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn strip_web_attachment_footer() {
        let s = "[pep] Line one\n\n[doc.pdf](/api/uploads/web/1/x.pdf)";
        assert_eq!(strip_send_message_echo(s), "Line one");
    }

    #[test]
    fn strip_telegram_attachment_echo() {
        let s = "[attachment:/tmp/a.pdf] Caption here";
        assert_eq!(strip_send_message_echo(s), "Caption here");
    }

    #[test]
    fn plan_suffix_only_when_memory_tail() {
        // Need > MIN_PRIOR_WORDS tokens in the caption (five "Hello world..." lines are only ~30).
        let last = msg("[pep] Hello world repeated text for length. \
             Hello world repeated text for length. \
             Hello world repeated text for length. \
             Hello world repeated text for length. \
             Hello world repeated text for length. \
             one two three four five six seven eight.\n\n[doc.pdf](/api/uploads/web/1/x.pdf)");
        // Main body must stay highly similar to the prior caption (token-bag dice ≥ SIMILARITY_THRESHOLD).
        let final_txt = "[pep] Hello world repeated text for length. \
             Hello world repeated text for length. \
             Hello world repeated text for length. \
             Hello world repeated text for length. \
             Hello world repeated text for length. \
             one two three four five six seven eight.\n\n---\n**Memory Update**:\n- tier3 note";
        let plan = plan_agent_final_delivery(Some(&last), final_txt);
        match plan {
            AgentFinalDeliveryPlan::DeliverSuffixOnly(s) => {
                assert!(s.contains("Memory"));
                assert!(s.contains("tier3"));
            }
            other => panic!("expected suffix, got {other:?}"),
        }
    }

    #[test]
    fn plan_full_when_short_prior_ping() {
        let last = msg("[pep] Short ping only\n\n[a](/api/uploads/web/1/x.pdf)");
        let plan =
            plan_agent_final_delivery(Some(&last), "[pep] Long different final ".repeat(20).trim());
        assert_eq!(plan, AgentFinalDeliveryPlan::DeliverFull);
    }

    #[test]
    fn plan_skip_exact_duplicate_indicated() {
        let binding = "[pep] Same body words ".repeat(10);
        let body = binding.trim();
        let stored = format!("{body}\n\n[f](/api/uploads/web/1/y.pdf)");
        let last = msg(&stored);
        // Model returns the same persona-prefixed body as already stored (no new tail).
        let plan = plan_agent_final_delivery(Some(&last), &stored);
        assert_eq!(plan, AgentFinalDeliveryPlan::Skip);
    }

    #[test]
    fn find_anchor_skips_newer_non_send_message_bot_row() {
        let ts = Utc::now().to_rfc3339();
        let send_body = format!(
            "{} {}",
            "[pep] alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega one. ",
            "[pep] alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega two.\n\n[x](/api/uploads/web/1/a.pdf)"
        );
        let send = StoredMessage {
            id: "a".into(),
            chat_id: 1,
            persona_id: 3,
            sender_name: "b".into(),
            content: send_body,
            is_from_bot: true,
            timestamp: ts.clone(),
        };
        let noise = StoredMessage {
            id: "b".into(),
            chat_id: 1,
            persona_id: 3,
            sender_name: "b".into(),
            content: "[pep] tier3 memory patch applied".into(),
            is_from_bot: true,
            timestamp: ts,
        };
        let recent = vec![send, noise];
        let anchor = find_send_message_dedupe_anchor(&recent, Utc::now(), 120);
        assert_eq!(anchor.map(|m| m.id.as_str()), Some("a"));
    }

    #[test]
    fn plan_skip_paraphrase_no_tail() {
        let last = msg(
            "[pep] Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega repeat. \
             Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega again.\n\n[f](/api/uploads/web/1/y.pdf)",
        );
        let final_txt = "[pep] Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega restate. \
             Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega restate2.";
        let plan = plan_agent_final_delivery(Some(&last), final_txt);
        assert_eq!(plan, AgentFinalDeliveryPlan::Skip);
    }
}
