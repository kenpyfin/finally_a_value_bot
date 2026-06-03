//! Enforce `activate_skill` for workflow authoring skills before mutating workflow YAML.

pub const REQUIRED_CREATE_WORKFLOW_SKILL: &str = "create-workflow";
pub const REQUIRED_MODIFY_WORKFLOW_SKILL: &str = "modify-workflow";

const WORKFLOW_WRITE_TOOLS: &[&str] = &["write_workflow"];

pub fn tool_name_writes_workflows(tool_name: &str) -> bool {
    WORKFLOW_WRITE_TOOLS
        .iter()
        .any(|t| t.eq_ignore_ascii_case(tool_name))
}

pub fn extract_workflow_id_from_write_input(input: &serde_json::Value) -> Option<String> {
    input
        .get("workflow_id")
        .or_else(|| input.get("id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn requires_create_workflow_activation(
    tool_name: &str,
    _input: &serde_json::Value,
    workflow_exists: bool,
) -> bool {
    if !tool_name_writes_workflows(tool_name) {
        return false;
    }
    !workflow_exists
}

pub fn requires_modify_workflow_activation(
    tool_name: &str,
    _input: &serde_json::Value,
    workflow_exists: bool,
) -> bool {
    if !tool_name_writes_workflows(tool_name) {
        return false;
    }
    workflow_exists
}

pub fn create_workflow_required_error_message() -> String {
    format!(
        "Creating a new workflow requires activating `{REQUIRED_CREATE_WORKFLOW_SKILL}` first in this turn. \
         Call `activate_skill` with skill_name `{REQUIRED_CREATE_WORKFLOW_SKILL}`, then use `write_workflow`."
    )
}

pub fn modify_workflow_required_error_message() -> String {
    format!(
        "Changing an existing workflow requires activating `{REQUIRED_MODIFY_WORKFLOW_SKILL}` first in this turn. \
         Call `activate_skill` with skill_name `{REQUIRED_MODIFY_WORKFLOW_SKILL}`, read the workflow with `read_workflow`, then `write_workflow`."
    )
}
