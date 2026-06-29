use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use crate::runtime_toggles::RuntimeToggles;

use crate::claude::ToolDefinition;
use crate::safety_redaction::EnvSecretRedactor;
use crate::tools::command_runner::{build_command_with_env, shell_command};

use super::bash_safety::{
    check_bash_safety, check_expensive_shell_search, is_expensive_shell_search,
    parse_confirmation_prefix, EXPENSIVE_SHELL_SEARCH_TIMEOUT_SECS,
};
use super::{schema_object, Tool, ToolResult};

fn maybe_rewrite_leading_tool_path(
    workspace_root: &std::path::Path,
    working_dir: &std::path::Path,
    command: &str,
) -> Option<String> {
    let trimmed = command.trim_start();
    let mut parts = trimmed.split_whitespace();
    let launcher = parts.next()?;
    if !matches!(
        launcher,
        "python" | "python3" | "node" | "bash" | "sh" | "pwsh" | "powershell"
    ) {
        return None;
    }
    let script = parts.next()?;
    if !(script.starts_with("skills/") || script.starts_with("runtime/")) {
        return None;
    }
    let resolved = super::resolve_tool_path(workspace_root, working_dir, script);
    Some(trimmed.replacen(script, &resolved.to_string_lossy(), 1))
}

pub struct BashTool {
    working_dir: PathBuf,
    safety_execution_mode: String,
    safety_risky_categories: Vec<String>,
    runtime_toggles: Arc<RuntimeToggles>,
    env_redactor: Arc<EnvSecretRedactor>,
}

impl BashTool {
    pub fn new(working_dir: &str) -> Self {
        Self::new_with_safety(
            working_dir,
            "warn_confirm".to_string(),
            vec![
                "destructive".into(),
                "system".into(),
                "network".into(),
                "package".into(),
            ],
            RuntimeToggles::new(false),
            Arc::new(EnvSecretRedactor::empty()),
        )
    }

    pub fn new_with_safety(
        working_dir: &str,
        safety_execution_mode: String,
        safety_risky_categories: Vec<String>,
        runtime_toggles: Arc<RuntimeToggles>,
        env_redactor: Arc<EnvSecretRedactor>,
    ) -> Self {
        Self {
            working_dir: PathBuf::from(working_dir),
            safety_execution_mode,
            safety_risky_categories,
            runtime_toggles,
            env_redactor,
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "bash",
            "Execute a bash command and return the output. Use for running shell commands, scripts, or system operations. Do not use recursive `grep -r` or unbounded `find` over large trees — use the `glob` and `grep` tools instead (shell recursive search is blocked unless you prefix CONFIRM_EXECUTE).",
            schema_object(
                json!({
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 3600)"
                    }
                }),
                &["command"],
            ),
        )
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let raw_command = match input.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("Missing 'command' parameter".into()),
        };
        let (confirmed, command) = parse_confirmation_prefix(raw_command.trim());

        let mut timeout_secs = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);
        if is_expensive_shell_search(&command) {
            timeout_secs = timeout_secs.min(EXPENSIVE_SHELL_SEARCH_TIMEOUT_SECS);
        }
        let auth = super::auth_context_from_input(&input);
        let working_dir =
            super::resolve_tool_working_dir_for_auth(&self.working_dir, auth.as_ref());
        if let Err(e) = tokio::fs::create_dir_all(&working_dir).await {
            return ToolResult::error(format!(
                "Failed to create working directory {}: {e}",
                working_dir.display()
            ));
        }

        if let Some(blocked) = check_bash_safety(
            &command,
            confirmed,
            &self.safety_execution_mode,
            &self.safety_risky_categories,
        ) {
            return blocked;
        }

        if let Some(blocked) = check_expensive_shell_search(&command, confirmed) {
            return blocked;
        }
        let command = maybe_rewrite_leading_tool_path(&self.working_dir, &working_dir, &command)
            .unwrap_or(command);

        info!("Executing bash: {}", self.env_redactor.redact(&command));

        let spec = shell_command(&command);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            build_command_with_env(
                &spec,
                Some(&working_dir),
                self.runtime_toggles.tool_output_debug(),
            )
            .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                let mut result_text = String::new();
                if !stdout.is_empty() {
                    result_text.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !result_text.is_empty() {
                        result_text.push('\n');
                    }
                    result_text.push_str("STDERR:\n");
                    result_text.push_str(&stderr);
                }
                if result_text.is_empty() {
                    result_text = format!("Command completed with exit code {exit_code}");
                }

                // Truncate very long output
                if result_text.len() > 30000 {
                    result_text.truncate(30000);
                    result_text.push_str("\n... (output truncated)");
                }

                if exit_code == 0 {
                    ToolResult::success(result_text).with_status_code(exit_code)
                } else {
                    ToolResult::error(format!("Exit code {exit_code}\n{result_text}"))
                        .with_status_code(exit_code)
                        .with_error_type("process_exit")
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("Failed to execute command: {e}"))
                .with_error_type("spawn_error"),
            Err(_) => ToolResult::error(format!("Command timed out after {timeout_secs} seconds"))
                .with_error_type("timeout"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Windows `canonicalize()` returns `\\?\`-prefixed paths; PowerShell prints normal `C:\...`.
    /// Normalize both for substring comparison.
    fn normalize_path_for_shell_assert(path: &str) -> String {
        let mut s = path.replace('\\', "/").to_lowercase();
        if cfg!(windows) {
            if let Some(rest) = s.strip_prefix("//?/") {
                s = rest.to_string();
            }
        }
        s
    }

    #[test]
    fn test_rewrite_leading_tool_path_for_skills() {
        let workspace = std::path::PathBuf::from("/tmp/workspace");
        let persona = workspace
            .join("shared")
            .join("personas")
            .join("1")
            .join("1");
        let rewritten = maybe_rewrite_leading_tool_path(
            &workspace,
            &persona,
            "python3 skills/read-email/read_email_tool.py --help",
        )
        .expect("rewrite expected");
        let normalized = rewritten.replace('\\', "/");
        assert!(normalized.contains("skills/read-email/read_email_tool.py"));
    }

    #[tokio::test]
    async fn test_bash_echo() {
        let tool = BashTool::new(".");
        let result = tool.execute(json!({"command": "echo hello"})).await;
        assert!(!result.is_error);
        assert!(result.content.contains("hello"));
    }

    #[tokio::test]
    async fn test_bash_exit_code_nonzero() {
        let tool = BashTool::new(".");
        let result = tool.execute(json!({"command": "exit 1"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("Exit code 1"));
    }

    #[tokio::test]
    async fn test_bash_stderr() {
        let tool = BashTool::new(".");
        // Unix: sh -c; Windows: PowerShell (see shell_command) — use shell-appropriate stderr.
        let cmd = if cfg!(windows) {
            "[Console]::Error.WriteLine('err')"
        } else {
            "echo err >&2"
        };
        let result = tool.execute(json!({"command": cmd})).await;
        assert!(!result.is_error); // exit code is 0
        assert!(result.content.contains("STDERR"));
        assert!(result.content.contains("err"));
    }

    #[tokio::test]
    async fn test_bash_timeout() {
        let tool = BashTool::new(".");
        let result = tool
            .execute(json!({"command": "sleep 10", "timeout_secs": 1}))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("timed out"));
    }

    #[tokio::test]
    async fn test_bash_missing_command() {
        let tool = BashTool::new(".");
        let result = tool.execute(json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("Missing 'command'"));
    }

    #[test]
    fn test_bash_tool_name_and_definition() {
        let tool = BashTool::new(".");
        assert_eq!(tool.name(), "bash");
        let def = tool.definition();
        assert_eq!(def.name, "bash");
        assert!(!def.description.is_empty());
        assert!(def.input_schema["properties"]["command"].is_object());
    }

    #[tokio::test]
    async fn test_bash_uses_working_dir() {
        let root =
            std::env::temp_dir().join(format!("finally_a_value_bot_bash_{}", uuid::Uuid::new_v4()));
        let work = root.join("workspace");
        std::fs::create_dir_all(&work).unwrap();

        let tool = BashTool::new(work.to_str().unwrap());
        // PowerShell's `pwd` can format as a table; use a plain path string on Windows.
        let cmd = if cfg!(windows) {
            "(Get-Location).Path"
        } else {
            "pwd"
        };
        let result = tool.execute(json!({"command": cmd})).await;
        assert!(!result.is_error);
        // Actual cwd is workspace/shared (see resolve_tool_working_dir). Compare canonical paths so
        // Windows matches long paths (Get-Location) vs short temp segments (RUNNER~1).
        let expected = crate::tools::resolve_tool_working_dir(&work);
        let expected_path = expected.canonicalize().unwrap_or_else(|_| expected.clone());
        let expected_norm = normalize_path_for_shell_assert(&expected_path.to_string_lossy());
        let out_norm = normalize_path_for_shell_assert(result.content.trim());
        assert!(
            out_norm.contains(&expected_norm),
            "expected cwd in output.\nexpected: {expected_norm}\noutput:\n{}",
            result.content
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_bash_warn_confirm_requires_prefix_for_risky_command() {
        let tool = BashTool::new_with_safety(
            ".",
            "warn_confirm".into(),
            vec!["destructive".into()],
            RuntimeToggles::new(false),
        );
        let result = tool.execute(json!({"command": "rm -rf /tmp/foo"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("Execution paused by safety policy"));
        assert_eq!(result.error_type.as_deref(), Some("confirmation_required"));
    }

    #[tokio::test]
    async fn test_bash_warn_confirm_allows_risky_command_with_prefix() {
        let tool = BashTool::new_with_safety(
            ".",
            "warn_confirm".into(),
            vec!["destructive".into()],
            RuntimeToggles::new(false),
        );
        // Unix: real rm; Windows (PowerShell): keep `rm -rf` substring for destructive match but run a no-op.
        let cmd = if cfg!(windows) {
            "CONFIRM_EXECUTE Write-Output 'rm -rf noop'"
        } else {
            "CONFIRM_EXECUTE rm -rf /tmp/finally_a_value_bot_test_confirm"
        };
        let result = tool.execute(json!({"command": cmd})).await;
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_bash_strict_blocks_risky_command() {
        let tool = BashTool::new_with_safety(
            ".",
            "strict".into(),
            vec!["package".into()],
            RuntimeToggles::new(false),
        );
        let result = tool.execute(json!({"command": "npm install lodash"})).await;
        assert!(result.is_error);
        assert!(result
            .content
            .contains("Blocked by safety_execution_mode=strict"));
        assert_eq!(result.error_type.as_deref(), Some("blocked_by_policy"));
    }

    #[tokio::test]
    async fn test_bash_blocks_recursive_grep_without_confirm() {
        let tool = BashTool::new(".");
        let result = tool
            .execute(json!({"command": "grep -r foo ./shared/ | head -n 5"}))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("Blocked expensive shell search"));
        assert_eq!(result.error_type.as_deref(), Some("blocked_by_policy"));
    }
}
