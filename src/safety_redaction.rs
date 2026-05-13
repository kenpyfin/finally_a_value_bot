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

/// True when `rest` begins (after optional ASCII whitespace) with a known model-weight extension.
/// Used so long-token redaction does not strip LoRA / checkpoint basenames in tool echoes.
fn is_followed_by_model_weight_extension(rest: &str) -> bool {
    let s = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let Some(after_dot) = s.strip_prefix('.') else {
        return false;
    };
    let end = after_dot
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(after_dot.len());
    let ext = &after_dot[..end];
    ext.eq_ignore_ascii_case("safetensors")
        || ext.eq_ignore_ascii_case("ckpt")
        || ext.eq_ignore_ascii_case("pth")
        || ext.eq_ignore_ascii_case("bin")
        || ext.eq_ignore_ascii_case("onnx")
        || ext.eq_ignore_ascii_case("gguf")
        || ext.eq_ignore_ascii_case("ggml")
        || ext.eq_ignore_ascii_case("pt")
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

/// Known-secret patterns only: explicit env-like assignments, bearer headers, query secrets, common key prefixes.
/// No heuristic long-token masking — safe for assistant text shown to users (filenames and long benign strings stay intact).
pub fn redact_secrets_user_visible(text: &str) -> String {
    redact_targeted_secrets(text)
}

/// Same as targeted redaction plus a conservative long-token heuristic for logs, tool payloads, and internal prompts.
pub fn redact_secrets_internal(text: &str) -> String {
    apply_long_token_fallback(&redact_targeted_secrets(text))
}

fn redact_targeted_secrets(text: &str) -> String {
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

    redacted
}

fn apply_long_token_fallback(redacted: &str) -> String {
    let re = long_token_like_regex();
    let mut out = String::with_capacity(redacted.len());
    let mut last_end = 0usize;
    for m in re.find_iter(redacted) {
        out.push_str(&redacted[last_end..m.start()]);
        let token = m.as_str();
        let after = &redacted[m.end()..];
        if is_followed_by_model_weight_extension(after) || !likely_secret_token(token) {
            out.push_str(token);
        } else {
            out.push_str(REDACTED);
        }
        last_end = m.end();
    }
    out.push_str(&redacted[last_end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::{redact_secrets_internal, redact_secrets_user_visible, REDACTED};

    #[test]
    fn internal_redacts_long_token_via_fallback() {
        let input = "x=lLLWRw8DBX4S99wN4ra4XRlLC1nkpv30zPHoABCDEFGHIJKLMNOP"; // len >= 48, mixed
        let out = redact_secrets_internal(input);
        assert!(!out.contains("lLLWRw"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn user_visible_preserves_long_token_without_explicit_secret_context() {
        let input = "x=lLLWRw8DBX4S99wN4ra4XRlLC1nkpv30zPHoABCDEFGHIJKLMNOP";
        let out = redact_secrets_user_visible(input);
        assert_eq!(input, out);
    }

    #[test]
    fn user_visible_preserves_long_pdf_filename() {
        let name = "Capital_One_Senior_PM_Resume_v2_ABC987xyzABCDEFGHIJ_KLMNOP";
        assert!(name.len() >= 40);
        let input = format!("Generated {name}.pdf — review when ready.");
        let out = redact_secrets_user_visible(&input);
        assert!(out.contains(".pdf"));
        assert!(out.contains(name));
        assert!(!out.contains(REDACTED));
    }

    #[test]
    fn internal_can_redact_long_pdf_basename_fallback() {
        let name = "Capital_One_Senior_PM_Resume_v2_ABC987xyzABCDEFGHIJ_KLMNOP";
        let input = format!("Attachment: {name}.pdf");
        let out = redact_secrets_internal(&input);
        assert!(out.contains(".pdf")); // punctuation splits word boundary typically before .
        assert!(!out.contains(name));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn internal_preserves_long_lora_basename_before_safetensors() {
        let basename = "pz_face_character_lora_v3_final_mix_abc123def456ghi789jkl012mno";
        assert!(basename.len() >= 40);
        let input = format!("remember the correct LoRA is {basename}.safetensors for the swap job");
        let out = redact_secrets_internal(&input);
        assert!(
            out.contains(basename),
            "expected LoRA basename preserved, got: {out}"
        );
        assert!(out.contains(".safetensors"));
        assert!(!out.contains(REDACTED));
    }

    #[test]
    fn redacts_apify_tokens_via_query_param_in_both_modes() {
        let input = "token=test_api_lLLWRw8DBX4S99wN4ra4XRlLC1nkpv30zPHo";
        let out_vis = redact_secrets_user_visible(input);
        let out_int = redact_secrets_internal(input);
        assert!(!out_vis.contains("test_api_"));
        assert!(out_vis.contains(REDACTED));
        assert!(!out_int.contains("test_api_"));
        assert!(out_int.contains(REDACTED));
    }

    #[test]
    fn redacts_secret_assignments_and_bearer_headers_in_user_visible() {
        let input = "API_KEY=sk-proj-1234567890abcdefghijklmno\nAuthorization: Bearer abcdefghijklmnopqrstuvwxyz123456";
        let output = redact_secrets_user_visible(input);
        assert!(!output.contains("sk-proj-1234567890abcdefghijklmno"));
        assert!(!output.contains("abcdefghijklmnopqrstuvwxyz123456"));
        assert!(output.contains("API_KEY=[REDACTED_SECRET]"));
        assert!(output.contains("Authorization: Bearer [REDACTED_SECRET]"));
    }

    #[test]
    fn redacts_query_param_tokens_in_user_visible() {
        let input = "https://example.com?api_key=secretvalue123456&ok=1";
        let output = redact_secrets_user_visible(input);
        assert!(output.contains("api_key=[REDACTED_SECRET]"));
        assert!(output.contains("&ok=1"));
    }

    #[test]
    fn keeps_short_non_secret_values_user_visible() {
        let input = "api key label, token-ish word: keyboard";
        let output = redact_secrets_user_visible(input);
        assert_eq!(input, output);
    }

    #[test]
    fn internal_redacts_via_literal_prefix() {
        let input = "k=ghp_fake123456789012345678901234567890";
        let out = redact_secrets_internal(input);
        assert!(!out.contains("ghp_"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn user_visible_redacts_via_literal_prefix() {
        let input = "k=ghp_fake123456789012345678901234567890";
        let out = redact_secrets_user_visible(input);
        assert!(!out.contains("ghp_"));
        assert!(out.contains(REDACTED));
    }
}
