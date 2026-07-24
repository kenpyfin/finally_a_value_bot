use std::path::Path;

pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

pub fn shell_command(command: &str) -> CommandSpec {
    if cfg!(target_os = "windows") {
        CommandSpec {
            program: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                command.to_string(),
            ],
        }
    } else {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());
        CommandSpec {
            program: shell,
            args: vec!["-c".to_string(), command.to_string()],
        }
    }
}

pub fn build_command(spec: &CommandSpec, working_dir: Option<&Path>) -> tokio::process::Command {
    build_command_with_env(spec, working_dir, false, None)
}

pub fn build_command_with_env(
    spec: &CommandSpec,
    working_dir: Option<&Path>,
    tool_output_debug: bool,
    git_ceiling_workspace: Option<&Path>,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(&spec.program);
    cmd.args(&spec.args);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    apply_tool_output_debug_env(&mut cmd, tool_output_debug);
    if let Some(workspace_root) = git_ceiling_workspace {
        crate::self_repo::apply_git_ceiling_env(&mut cmd, workspace_root);
    }
    cmd
}

/// Sets `TOOL_OUTPUT_DEBUG=1` for workspace PZ/ComfyUI scripts when debug logging is enabled.
pub fn apply_tool_output_debug_env(cmd: &mut tokio::process::Command, tool_output_debug: bool) {
    if tool_output_debug {
        cmd.env("TOOL_OUTPUT_DEBUG", "1");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_command_shape() {
        let spec = shell_command("echo hello");
        assert!(!spec.program.is_empty());
        assert!(!spec.args.is_empty());
    }
}
