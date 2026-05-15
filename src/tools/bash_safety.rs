//! Shared bash safety checks for `bash` and `spawn_background_command`.

use super::ToolResult;

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
