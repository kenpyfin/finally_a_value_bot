use serde_json::{json, Value};

use super::schema::WorkflowStepOutcome;

#[derive(Debug, Clone)]
pub struct WorkflowExecutionContext {
    pub inputs: Value,
    pub steps: serde_json::Map<String, Value>,
    pub vars: serde_json::Map<String, Value>,
}

impl WorkflowExecutionContext {
    pub fn new(inputs: Value) -> Self {
        Self {
            inputs,
            steps: serde_json::Map::new(),
            vars: serde_json::Map::new(),
        }
    }

    pub fn as_template_value(&self) -> Value {
        json!({
            "inputs": self.inputs,
            "steps": self.steps,
            "vars": self.vars,
        })
    }

    pub fn record_step(&mut self, id: &str, outcome: &WorkflowStepOutcome) {
        self.steps.insert(
            id.to_string(),
            json!({
                "stdout": outcome.stdout,
                "is_error": outcome.is_error,
                "duration_ms": outcome.duration_ms,
                "error_type": outcome.error_type,
            }),
        );
    }

    pub fn set_var(&mut self, name: &str, value: String) {
        self.vars.insert(name.to_string(), Value::String(value));
    }
}
