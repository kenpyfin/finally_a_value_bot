use regex::{Captures, Regex};
use std::sync::OnceLock;

const REDACTED: &str = "[REDACTED_SECRET]";

fn literal_secret_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"\bapify_api_[A-Za-z0-9]{16,}\b").expect("valid apify token regex"),
            Regex::new(r"\bsk-[A-Za-z0-9_-]{16,}\b").expect("valid sk token regex"),
            Regex::new(r"\bghp_[A-Za-z0-9]{20,}\b").expect("valid github token regex"),
            Regex::new(r"\bAIza[0-9A-Za-z_-]{20,}\b").expect("valid google api key regex"),
        ]
    })
}

fn auth_header_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?i)\b(authorization\s*:\s*bearer\s+)([A-Za-z0-9._\-]{12,})"#)
            .expect("valid bearer regex")
    })
}

fn query_secret_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?i)([?&](?:api[_-]?key|token|access[_-]?token|authorization)=)([^&\s"'`]+)"#)
            .expect("valid query secret regex")
    })
}

fn assignment_secret_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?i)\b([A-Z][A-Z0-9_]*(?:TOKEN|SECRET|API_KEY|PASSWORD|PASS|PRIVATE_KEY|ACCESS_KEY|AUTH)\b\s*[:=]\s*)(['"]?)([^,\s'"`]{6,})(['"]?)"#,
        )
        .expect("valid assignment regex")
    })
}

fn long_token_like_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\b[A-Za-z0-9_-]{40,}\b").expect("valid long token regex"))
}

fn likely_secret_token(token: &str) -> bool {
    let has_letter = token.chars().any(|c| c.is_ascii_alphabetic());
    let has_digit = token.chars().any(|c| c.is_ascii_digit());
    let punctuation_count = token
        .chars()
        .filter(|c| *c == '_' || *c == '-' || *c == '.')
        .count();
    has_letter && has_digit && (token.len() >= 48 || punctuation_count >= 2)
}

pub fn redact_secrets(text: &str) -> String {
    let mut redacted = text.to_string();

    for pattern in literal_secret_patterns() {
        redacted = pattern.replace_all(&redacted, REDACTED).into_owned();
    }

    redacted = auth_header_regex()
        .replace_all(&redacted, |caps: &Captures<'_>| {
            format!("{}{}", &caps[1], REDACTED)
        })
        .into_owned();
    redacted = query_secret_regex()
        .replace_all(&redacted, |caps: &Captures<'_>| {
            format!("{}{}", &caps[1], REDACTED)
        })
        .into_owned();
    redacted = assignment_secret_regex()
        .replace_all(&redacted, |caps: &Captures<'_>| {
            format!("{}{}{}{}", &caps[1], &caps[2], REDACTED, &caps[4])
        })
        .into_owned();
    redacted = long_token_like_regex()
        .replace_all(&redacted, |caps: &Captures<'_>| {
            let token = &caps[0];
            if likely_secret_token(token) {
                REDACTED.to_string()
            } else {
                token.to_string()
            }
        })
        .into_owned();

    redacted
}

#[cfg(test)]
mod tests {
    use super::redact_secrets;

    #[test]
    fn redacts_apify_tokens() {
        let input = "token=test_api_lLLWRw8DBX4S99wN4ra4XRlLC1nkpv30zPHo";
        let output = redact_secrets(input);
        assert!(!output.contains("test_api_"));
        assert!(output.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn redacts_secret_assignments_and_bearer_headers() {
        let input = "API_KEY=sk-proj-1234567890abcdefghijklmno\nAuthorization: Bearer abcdefghijklmnopqrstuvwxyz123456";
        let output = redact_secrets(input);
        assert!(!output.contains("sk-proj-1234567890abcdefghijklmno"));
        assert!(!output.contains("abcdefghijklmnopqrstuvwxyz123456"));
        assert!(output.contains("API_KEY=[REDACTED_SECRET]"));
        assert!(output.contains("Authorization: Bearer [REDACTED_SECRET]"));
    }

    #[test]
    fn redacts_query_param_tokens() {
        let input = "https://example.com?api_key=secretvalue123456&ok=1";
        let output = redact_secrets(input);
        assert!(output.contains("api_key=[REDACTED_SECRET]"));
        assert!(output.contains("&ok=1"));
    }

    #[test]
    fn keeps_short_non_secret_values() {
        let input = "api key label, token-ish word: keyboard";
        let output = redact_secrets(input);
        assert_eq!(input, output);
    }
}
