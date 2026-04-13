use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use tracing::info;

use crate::claude::ToolDefinition;
use crate::tools::command_runner::{build_command, shell_command};

use super::{schema_object, Tool, ToolResult};

pub struct BashTool {
    working_dir: PathBuf,
    safety_execution_mode: String,
    safety_risky_categories: Vec<String>,
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
        )
    }

    pub fn new_with_safety(
        working_dir: &str,
        safety_execution_mode: String,
        safety_risky_categories: Vec<String>,
    ) -> Self {
        Self {
            working_dir: PathBuf::from(working_dir),
            safety_execution_mode,
            safety_risky_categories,
        }
    }
}

fn parse_confirmation_prefix(command: &str) -> (bool, String) {
    const PREFIX: &str = "CONFIRM_EXECUTE ";
    if let Some(rest) = command.strip_prefix(PREFIX) {
        (true, rest.trim().to_string())
    } else {
        (false, command.to_string())
    }
}

fn command_is_risky_for_category(command_lower: &str, category: &str) -> bool {
    match category {
        "destructive" => {
            command_lower.contains("rm -rf")
                || command_lower.contains("rm -fr")
                || command_lower.contains("mkfs")
                || command_lower.contains("shred ")
                || command_lower.contains(" dd if=")
                || command_lower.starts_with("dd if=")
        }
        "system" => {
            command_lower.contains("systemctl ")
                || command_lower.contains(" service ")
                || command_lower.starts_with("service ")
                || command_lower.contains("shutdown")
                || command_lower.contains("reboot")
                || command_lower.contains("killall ")
                || command_lower.contains("pkill ")
                || command_lower.contains("launchctl ")
                || command_lower.contains("sudo ")
        }
        "network" => {
            (command_lower.contains("curl ")
                && (command_lower.contains(" -x post")
                    || command_lower.contains(" --request post")
                    || command_lower.contains(" -x put")
                    || command_lower.contains(" --request put")
                    || command_lower.contains(" -x patch")
                    || command_lower.contains(" --request patch")
                    || command_lower.contains(" -x delete")
                    || command_lower.contains(" --request delete")))
                || (command_lower.contains("wget ")
                    && (command_lower.contains(" --post")
                        || command_lower.contains(" --method=post")
                        || command_lower.contains(" --method=put")
                        || command_lower.contains(" --method=patch")
                        || command_lower.contains(" --method=delete")))
        }
        "package" => {
            command_lower.contains("apt-get ")
                || command_lower.starts_with("apt ")
                || command_lower.contains(" yum ")
                || command_lower.starts_with("yum ")
                || command_lower.contains(" dnf ")
                || command_lower.starts_with("dnf ")
                || command_lower.contains(" pacman ")
                || command_lower.starts_with("pacman ")
                || command_lower.contains("brew install")
                || command_lower.contains("brew uninstall")
                || command_lower.contains("pip install")
                || command_lower.contains("pip uninstall")
                || command_lower.contains("npm install")
                || command_lower.contains("npm uninstall")
                || command_lower.contains("cargo install")
                || command_lower.contains("cargo uninstall")
        }
        _ => false,
    }
}

fn detect_risky_categories(command: &str, configured_categories: &[String]) -> Vec<String> {
    let command_lower = command.to_ascii_lowercase();
    let mut matched = Vec::new();
    for category in configured_categories {
        let c = category.trim().to_ascii_lowercase();
        if c.is_empty() {
            continue;
        }
        if command_is_risky_for_category(&command_lower, &c) {
            matched.push(c);
        }
    }
    matched.sort();
    matched.dedup();
    matched
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".into(),
            description: "Execute a bash command and return the output. Use for running shell commands, scripts, or system operations.".into(),
            input_schema: schema_object(
                json!({
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 1500)"
                    }
                }),
                &["command"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let raw_command = match input.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("Missing 'command' parameter".into()),
        };
        let (confirmed, command) = parse_confirmation_prefix(raw_command.trim());

        let timeout_secs = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(1500);
        let working_dir = super::resolve_tool_working_dir(&self.working_dir);
        if let Err(e) = tokio::fs::create_dir_all(&working_dir).await {
            return ToolResult::error(format!(
                "Failed to create working directory {}: {e}",
                working_dir.display()
            ));
        }

        let safety_mode = self.safety_execution_mode.trim().to_ascii_lowercase();
        let risky_categories = detect_risky_categories(&command, &self.safety_risky_categories);
        if !risky_categories.is_empty() && safety_mode != "off" {
            if safety_mode == "strict" {
                return ToolResult::error(format!(
                    "Blocked by safety_execution_mode=strict. Risky categories detected: [{}]. Command was not executed.",
                    risky_categories.join(", ")
                ))
                .with_error_type("blocked_by_policy");
            }
            if safety_mode == "warn_confirm" && !confirmed {
                return ToolResult::error(format!(
                    "Execution paused by safety policy. Risky categories detected: [{}]. \
Add explicit confirmation by re-running with prefix: CONFIRM_EXECUTE <your command>",
                    risky_categories.join(", ")
                ))
                .with_error_type("confirmation_required");
            }
        }

        info!("Executing bash: {}", command);

        let spec = shell_command(&command);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            build_command(&spec, Some(&working_dir)).output(),
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
        let work_norm = work.to_string_lossy().replace('\\', "/");
        let out_norm = result.content.replace('\\', "/");
        assert!(out_norm.contains(&work_norm));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_bash_warn_confirm_requires_prefix_for_risky_command() {
        let tool =
            BashTool::new_with_safety(".", "warn_confirm".into(), vec!["destructive".into()]);
        let result = tool.execute(json!({"command": "rm -rf /tmp/foo"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("Execution paused by safety policy"));
        assert_eq!(result.error_type.as_deref(), Some("confirmation_required"));
    }

    #[tokio::test]
    async fn test_bash_warn_confirm_allows_risky_command_with_prefix() {
        let tool =
            BashTool::new_with_safety(".", "warn_confirm".into(), vec!["destructive".into()]);
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
        let tool = BashTool::new_with_safety(".", "strict".into(), vec!["package".into()]);
        let result = tool.execute(json!({"command": "npm install lodash"})).await;
        assert!(result.is_error);
        assert!(result
            .content
            .contains("Blocked by safety_execution_mode=strict"));
        assert_eq!(result.error_type.as_deref(), Some("blocked_by_policy"));
    }
}
