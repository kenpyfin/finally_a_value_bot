use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowScope {
    #[default]
    Global,
    Persona,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: String,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub inputs: BTreeMap<String, WorkflowInputDef>,
    pub steps: Vec<WorkflowStep>,
    #[serde(default = "default_on_error")]
    pub on_error: String,
}

fn default_version() -> u32 {
    1
}

fn default_enabled() -> bool {
    true
}

fn default_on_error() -> String {
    "fail".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInputDef {
    #[serde(rename = "type", default = "default_input_type")]
    pub input_type: String,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

fn default_input_type() -> String {
    "string".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WorkflowStep {
    Tool {
        id: String,
        tool: String,
        #[serde(default)]
        input: serde_json::Value,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    Script {
        id: String,
        skill_name: String,
        script: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        interpreter: Option<String>,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    Bash {
        id: String,
        command: String,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    Set {
        id: String,
        var: String,
        value: String,
    },
    Deliver {
        id: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        send_message: bool,
        #[serde(default)]
        message_input: Option<serde_json::Value>,
    },
}

impl WorkflowStep {
    pub fn id(&self) -> &str {
        match self {
            WorkflowStep::Tool { id, .. }
            | WorkflowStep::Script { id, .. }
            | WorkflowStep::Bash { id, .. }
            | WorkflowStep::Set { id, .. }
            | WorkflowStep::Deliver { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowListEntry {
    pub id: String,
    pub description: String,
    pub enabled: bool,
    pub version: u32,
    pub step_count: usize,
    pub scope: WorkflowScope,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepOutcome {
    pub id: String,
    pub step_type: String,
    pub is_error: bool,
    pub stdout: String,
    pub duration_ms: u128,
    pub error_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunReport {
    pub workflow_id: String,
    pub success: bool,
    pub steps: Vec<WorkflowStepOutcome>,
    pub deliver_text: String,
    pub error: Option<String>,
}

pub fn validate_workflow_id(id: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("workflow id must not be empty".into());
    }
    if trimmed.len() > 128 {
        return Err("workflow id must be at most 128 characters".into());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(
            "workflow id may only contain ASCII letters, digits, hyphens, and underscores".into(),
        );
    }
    Ok(())
}

pub fn validate_definition(def: &WorkflowDefinition) -> Result<(), String> {
    validate_workflow_id(&def.id)?;
    if def.steps.is_empty() {
        return Err("workflow must define at least one step".into());
    }
    let mut seen = std::collections::HashSet::new();
    for step in &def.steps {
        let sid = step.id();
        if sid.trim().is_empty() {
            return Err("each step must have a non-empty id".into());
        }
        if !seen.insert(sid.to_string()) {
            return Err(format!("duplicate step id '{sid}'"));
        }
        match step {
            WorkflowStep::Tool { tool, .. } if tool.trim().is_empty() => {
                return Err(format!("step '{sid}' tool name must not be empty"));
            }
            WorkflowStep::Script {
                skill_name, script, ..
            } if skill_name.trim().is_empty() || script.trim().is_empty() => {
                return Err(format!("step '{sid}' requires skill_name and script"));
            }
            WorkflowStep::Bash { command, .. } if command.trim().is_empty() => {
                return Err(format!("step '{sid}' bash command must not be empty"));
            }
            WorkflowStep::Set { var, .. } if var.trim().is_empty() => {
                return Err(format!("step '{sid}' set var must not be empty"));
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn parse_workflow_yaml(yaml: &str) -> Result<WorkflowDefinition, String> {
    let def: WorkflowDefinition =
        serde_yaml::from_str(yaml).map_err(|e| format!("invalid workflow YAML: {e}"))?;
    validate_definition(&def)?;
    Ok(def)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_workflow() {
        let yaml = r#"
id: demo
description: test
steps:
  - id: hi
    type: deliver
    text: hello
"#;
        let def = parse_workflow_yaml(yaml).unwrap();
        assert_eq!(def.id, "demo");
        assert_eq!(def.steps.len(), 1);
    }
}
