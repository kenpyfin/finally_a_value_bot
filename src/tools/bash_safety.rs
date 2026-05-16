//! Shared bash safety checks for `bash` and `spawn_background_command`.

use super::ToolResult;

/// Max runtime for expensive shell searches when explicitly confirmed via `CONFIRM_EXECUTE`.
pub const EXPENSIVE_SHELL_SEARCH_TIMEOUT_SECS: u64 = 120;

const EXPENSIVE_SEARCH_BLOCK_MESSAGE: &str = "Blocked expensive shell search. Recursive `grep -r` / unbounded `find` over large trees can run for many minutes and block the chat. \
Prefer: `glob` (file names), the `grep` tool (contents; skips binaries and huge dirs), `read_tiered_memory` / `list_cursor_agent_runs` (job status), or `read_file` on a known path. \
To run this exact shell command anyway, prefix: CONFIRM_EXECUTE <command>";

pub fn parse_confirmation_prefix(command: &str) -> (bool, String) {
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

pub fn detect_risky_categories(command: &str, configured_categories: &[String]) -> Vec<String> {
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

/// Returns `None` when execution may proceed, or a blocked/confirmation `ToolResult`.
pub fn check_bash_safety(
    command: &str,
    confirmed: bool,
    safety_execution_mode: &str,
    safety_risky_categories: &[String],
) -> Option<ToolResult> {
    let safety_mode = safety_execution_mode.trim().to_ascii_lowercase();
    let risky_categories = detect_risky_categories(command, safety_risky_categories);
    if risky_categories.is_empty() || safety_mode == "off" {
        return None;
    }
    if safety_mode == "strict" {
        return Some(
            ToolResult::error(format!(
                "Blocked by safety_execution_mode=strict. Risky categories detected: [{}]. Command was not executed.",
                risky_categories.join(", ")
            ))
            .with_error_type("blocked_by_policy"),
        );
    }
    if safety_mode == "warn_confirm" && !confirmed {
        return Some(
            ToolResult::error(format!(
                "Execution paused by safety policy. Risky categories detected: [{}]. \
Add explicit confirmation by re-running with prefix: CONFIRM_EXECUTE <your command>",
                risky_categories.join(", ")
            ))
            .with_error_type("confirmation_required"),
        );
    }
    None
}

/// True when a shell command is likely to scan huge directory trees (e.g. `grep -r` on `shared/`).
pub fn is_expensive_shell_search(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() {
        return false;
    }
    is_recursive_grep_command(cmd)
        || is_unbounded_find_command(cmd)
        || is_unbounded_ripgrep_command(cmd)
}

/// Returns `None` when execution may proceed, or a blocked `ToolResult`.
/// `spawn_background_command` does not call this — long scans may be intentional in background.
pub fn check_expensive_shell_search(command: &str, confirmed: bool) -> Option<ToolResult> {
    if confirmed || !is_expensive_shell_search(command) {
        return None;
    }
    Some(
        ToolResult::error(EXPENSIVE_SEARCH_BLOCK_MESSAGE.to_string())
            .with_error_type("blocked_by_policy"),
    )
}

fn is_recursive_grep_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    if !lower.contains("grep") {
        return false;
    }
    lower.contains(" -r")
        || lower.contains(" -rn")
        || lower.contains(" -r ")
        || lower.contains(" -rn ")
        || lower.contains(" --recursive")
        || lower.starts_with("grep -r")
        || lower.starts_with("grep -rn")
        || lower.starts_with("grep -R")
}

fn is_unbounded_find_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    if !lower.contains("find ") {
        return false;
    }
    if lower.contains("-maxdepth") {
        return false;
    }
    // Name-filtered finds (e.g. PZ-*.png) are usually fast.
    if lower.contains("-name") {
        return false;
    }
    true
}

fn is_unbounded_ripgrep_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let uses_rg = lower.starts_with("rg ") || lower.contains(" rg ");
    if !uses_rg {
        return false;
    }
    let bounded = lower.contains("--max-count")
        || lower.contains(" -m ")
        || lower.contains("| head ")
        || lower.contains("|head ");
    !bounded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expensive_shell_search_blocks_recursive_grep() {
        assert!(is_expensive_shell_search(
            "grep -r \"PZ\" /home/ken/proj/workspace/shared/ | head -n 10"
        ));
    }

    #[test]
    fn expensive_shell_search_allows_find_with_name_filter() {
        assert!(!is_expensive_shell_search(
            "find /tmp/shared -name 'PZ-*.png' -mtime -1"
        ));
    }

    #[test]
    fn expensive_shell_search_blocks_unbounded_find() {
        assert!(is_expensive_shell_search(
            "find ./workspace/shared -mtime -1"
        ));
    }

    #[test]
    fn expensive_shell_search_allows_simple_ls() {
        assert!(!is_expensive_shell_search(
            "ls -lh /tmp/shared/PZ-20260515-CAFE-LoRA-Fixed.png"
        ));
    }

    #[test]
    fn check_expensive_shell_search_requires_confirm_prefix() {
        let cmd = "grep -r foo ./shared/";
        assert!(check_expensive_shell_search(cmd, false).is_some());
        assert!(check_expensive_shell_search(cmd, true).is_none());
    }
}
