//! Literal secret redaction from env-like files on disk only (no regex heuristics).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::info;

use crate::builtin_skills;
use crate::config::Config;
use crate::tools::path_guard::is_env_like_name;

pub const REDACTED: &str = "[REDACTED_SECRET]";

const DEFAULT_MIN_VALUE_LEN: usize = 8;
const MAX_WALK_DEPTH: usize = 12;
const MAX_ENV_FILES: usize = 200;
const MAX_NEEDLES: usize = 500;
const MAX_ENV_FILE_BYTES: u64 = 256 * 1024;

const SKIP_DIR_NAMES: &[&str] = &[".git", "node_modules", "target", "runtime"];

const PLACEHOLDER_VALUES: &[&str] = &[
    "changeme",
    "change_me",
    "your_api_key",
    "your_api_key_here",
    "your-api-key",
    "insert_key_here",
    "replace_me",
    "placeholder",
    "xxx",
    "todo",
    "fixme",
    "example",
    "dummy",
    "test",
    "secret",
    "password",
];

/// Redacts only literal values parsed from env-like files at startup.
#[derive(Debug, Clone)]
pub struct EnvSecretRedactor {
    needles: Vec<String>,
}

impl EnvSecretRedactor {
    pub fn empty() -> Self {
        Self {
            needles: Vec::new(),
        }
    }

    pub fn discover(config: &Config) -> Self {
        let min_value_len = std::env::var("ENV_REDACT_MIN_VALUE_LEN")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .filter(|&n| (4..=128).contains(&n))
            .unwrap_or(DEFAULT_MIN_VALUE_LEN);

        let mut allowed_roots: Vec<PathBuf> = vec![config.workspace_root_absolute()];
        if let Ok(Some(config_env)) = Config::resolve_config_path() {
            if let Some(parent) = config_env.parent() {
                allowed_roots.push(parent.to_path_buf());
            }
            allowed_roots.push(config_env);
        }
        if let Some(builtin) = builtin_skills::resolve_builtin_skills_dir(config) {
            allowed_roots.push(builtin);
        }
        allowed_roots.sort();
        allowed_roots.dedup();

        let mut env_files: Vec<PathBuf> = Vec::new();
        let mut files_scanned = 0usize;

        if let Ok(Some(config_env)) = Config::resolve_config_path() {
            if config_env.is_file() && should_load_env_file(&config_env) {
                push_env_file(&mut env_files, config_env, &mut files_scanned);
            }
        }

        for root in &allowed_roots {
            if root.is_dir() {
                walk_for_env_files(root, &allowed_roots, 0, &mut env_files, &mut files_scanned);
            }
        }

        let mut value_set: HashSet<String> = HashSet::new();
        for path in &env_files {
            if value_set.len() >= MAX_NEEDLES {
                break;
            }
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    for (_key, value) in parse_env_content(&content) {
                        if value_set.len() >= MAX_NEEDLES {
                            break;
                        }
                        if should_redact_value(&value, min_value_len) {
                            value_set.insert(value);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "redaction",
                        path = %path.display(),
                        "Failed to read env file for redaction catalog: {e}"
                    );
                }
            }
        }

        let mut needles: Vec<String> = value_set.into_iter().collect();
        expand_url_encoded_needles(&mut needles);
        needles.sort_by_key(|s| std::cmp::Reverse(s.len()));
        needles.dedup();

        info!(
            target: "redaction",
            env_files = env_files.len(),
            needles = needles.len(),
            min_value_len = min_value_len,
            "Env-only secret redaction catalog built"
        );

        Self { needles }
    }

    pub fn redact(&self, text: &str) -> String {
        if self.needles.is_empty() {
            return text.to_string();
        }
        let mut out = text.to_string();
        for needle in &self.needles {
            if needle.is_empty() {
                continue;
            }
            if out.contains(needle) {
                out = out.replace(needle, REDACTED);
            }
        }
        out
    }

    #[cfg(test)]
    pub fn from_needles(needles: Vec<String>) -> Self {
        let mut needles = needles;
        needles.sort_by_key(|s| std::cmp::Reverse(s.len()));
        Self { needles }
    }
}

fn should_load_env_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| is_env_like_name(name) && !is_sample_or_example_env(name))
}

fn is_sample_or_example_env(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.contains(".example") || lower.contains(".sample")
}

fn push_env_file(files: &mut Vec<PathBuf>, path: PathBuf, count: &mut usize) {
    if *count >= MAX_ENV_FILES {
        return;
    }
    if files.iter().any(|p| p == &path) {
        return;
    }
    files.push(path);
    *count += 1;
}

fn path_under_allowed_roots(path: &Path, allowed_roots: &[PathBuf]) -> bool {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    allowed_roots.iter().any(|root| {
        let root_resolved = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        resolved.starts_with(&root_resolved)
    })
}

fn walk_for_env_files(
    dir: &Path,
    allowed_roots: &[PathBuf],
    depth: usize,
    out: &mut Vec<PathBuf>,
    files_scanned: &mut usize,
) {
    if depth > MAX_WALK_DEPTH || *files_scanned >= MAX_ENV_FILES {
        return;
    }
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in read_dir.flatten() {
        if *files_scanned >= MAX_ENV_FILES {
            return;
        }
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIR_NAMES.iter().any(|s| name == *s) {
                continue;
            }
            walk_for_env_files(&path, allowed_roots, depth + 1, out, files_scanned);
        } else if path.is_file() {
            if !is_env_like_name(&name) || is_sample_or_example_env(&name) {
                continue;
            }
            if std::fs::metadata(&path)
                .map(|m| m.len() <= MAX_ENV_FILE_BYTES)
                .unwrap_or(false)
                && path_under_allowed_roots(&path, allowed_roots)
            {
                push_env_file(out, path, files_scanned);
            }
        }
    }
}

fn parse_env_content(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let value = parse_env_value(raw_value.trim());
        if !value.is_empty() {
            pairs.push((key, value));
        }
    }
    pairs
}

fn parse_env_value(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if (raw.starts_with('"') && raw.ends_with('"'))
        || (raw.starts_with('\'') && raw.ends_with('\''))
    {
        let inner = &raw[1..raw.len().saturating_sub(1)];
        return inner.to_string();
    }
    raw.to_string()
}

fn should_redact_value(value: &str, min_len: usize) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < min_len {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if PLACEHOLDER_VALUES.iter().any(|p| lower == *p) {
        return false;
    }
    if lower.starts_with("your_") || lower.starts_with("replace_") || lower.contains("example.com")
    {
        return false;
    }
    true
}

fn expand_url_encoded_needles(needles: &mut Vec<String>) {
    let mut extra = Vec::new();
    for needle in needles.iter() {
        let encoded = urlencoding::encode(needle);
        if encoded != needle.as_str() {
            extra.push(encoded.into_owned());
        }
    }
    needles.extend(extra);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_env(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn redacts_literal_env_value() {
        let redactor = EnvSecretRedactor::from_needles(vec!["supersecret12345678".to_string()]);
        let out = redactor.redact("token=supersecret12345678 done");
        assert!(!out.contains("supersecret"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn preserves_linkedin_url_when_not_in_env() {
        let redactor = EnvSecretRedactor::empty();
        let url = "https://www.linkedin.com/jobs/view/4123789234567890123";
        assert_eq!(redactor.redact(url), url);
    }

    #[test]
    fn preserves_token_assignment_without_env_value() {
        let redactor = EnvSecretRedactor::empty();
        let input = "token=not_a_real_secret_value_abcdef";
        assert_eq!(redactor.redact(input), input);
    }

    #[test]
    fn preserves_long_filename_without_env_value() {
        let redactor = EnvSecretRedactor::empty();
        let name = "Capital_One_Senior_PM_Resume_v2_ABC987xyzABCDEFGHIJ_KLMNOP";
        let input = format!("Generated {name}.pdf — review when ready.");
        assert_eq!(redactor.redact(&input), input);
    }

    #[test]
    fn longest_needle_first_avoids_partial_leak() {
        let redactor = EnvSecretRedactor::from_needles(vec![
            "shortsec12".to_string(),
            "shortsec12345678".to_string(),
        ]);
        let out = redactor.redact("value=shortsec12345678");
        assert!(!out.contains("shortsec12"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn discover_loads_env_like_file() {
        let tmp = TempDir::new().unwrap();
        let skills = tmp.path().join("skills").join("foo");
        fs::create_dir_all(&skills).unwrap();
        write_env(&skills, ".env", "API_KEY=supersecret12345678\nSHORT=abc\n");

        let mut config = crate::config::test_config();
        config.workspace_dir = tmp.path().to_string_lossy().to_string();

        let redactor = EnvSecretRedactor::discover(&config);
        let out = redactor.redact("leak supersecret12345678 end");
        assert!(out.contains(REDACTED));
        assert!(!out.contains("supersecret12345678"));
        assert!(redactor.redact("SHORT=abc").contains("abc"));
    }

    #[test]
    fn parse_env_handles_quotes_and_export() {
        let pairs = parse_env_content(
            r#"
# comment
export FOO="quoted value here"
BAR=plain
"#,
        );
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].1, "quoted value here");
        assert_eq!(pairs[1].1, "plain");
    }
}
