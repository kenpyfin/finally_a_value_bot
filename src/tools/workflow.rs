use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde_json::json;

use crate::config::Config;
use crate::db::Database;
use crate::telegram::AppState;
use crate::workflow_engine::template::{default_inputs, merge_inputs};
use crate::workflow_engine::{
    execute_workflow, parse_workflow_yaml, WorkflowCatalog, WorkflowExecutionParams, WorkflowScope,
};

use super::{auth_context_from_input, schema_object, Tool, ToolRegistry, ToolResult};
use crate::claude::ToolDefinition;

fn catalog_from_config(config: &Config) -> WorkflowCatalog {
    WorkflowCatalog::new(
        config.workspace_root_absolute(),
        &config.workflow_definitions_dir,
        config.workflow_allow_persona_scope,
    )
}

fn scope_from_str(s: Option<&str>) -> WorkflowScope {
    match s.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("persona") => WorkflowScope::Persona,
        _ => WorkflowScope::Global,
    }
}

fn report_to_tool_result(report: &crate::workflow_engine::WorkflowRunReport) -> ToolResult {
    let body = serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".into());
    if report.success {
        ToolResult::success(body)
    } else {
        ToolResult::error(body).with_error_type("workflow_failed")
    }
}

pub struct ListWorkflowsTool {
    config: Arc<Config>,
}

impl ListWorkflowsTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for ListWorkflowsTool {
    fn name(&self) -> &str {
        "list_workflows"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_workflows".into(),
            description:
                "List authored deterministic workflows (YAML) available for this chat/persona."
                    .into(),
            input_schema: schema_object(json!({}), &[]),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        if !self.config.workflow_engine_enabled {
            return ToolResult::error(
                "Workflow engine is disabled (WORKFLOW_ENGINE_ENABLED=false).".into(),
            );
        }
        let auth = match auth_context_from_input(&input) {
            Some(a) => a,
            None => return ToolResult::error("Missing auth context".into()),
        };
        let catalog = catalog_from_config(&self.config);
        match catalog.list_entries(auth.caller_chat_id, auth.caller_persona_id) {
            Ok(entries) => {
                ToolResult::success(serde_json::to_string_pretty(&entries).unwrap_or_default())
            }
            Err(e) => ToolResult::error(e),
        }
    }
}

pub struct ReadWorkflowTool {
    config: Arc<Config>,
}

impl ReadWorkflowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for ReadWorkflowTool {
    fn name(&self) -> &str {
        "read_workflow"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_workflow".into(),
            description: "Read an authored workflow definition by id (parsed JSON + raw YAML)."
                .into(),
            input_schema: schema_object(
                json!({
                    "workflow_id": { "type": "string", "description": "Workflow id" },
                    "scope": { "type": "string", "description": "global or persona (default: resolve best match)" }
                }),
                &["workflow_id"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        if !self.config.workflow_engine_enabled {
            return ToolResult::error("Workflow engine is disabled.".into());
        }
        let workflow_id = match input.get("workflow_id").and_then(|v| v.as_str()) {
            Some(id) => id.trim(),
            None => return ToolResult::error("Missing workflow_id".into()),
        };
        let auth = match auth_context_from_input(&input) {
            Some(a) => a,
            None => return ToolResult::error("Missing auth context".into()),
        };
        let scope = scope_from_str(input.get("scope").and_then(|v| v.as_str()));
        let catalog = catalog_from_config(&self.config);
        match catalog.load(
            workflow_id,
            scope,
            auth.caller_chat_id,
            auth.caller_persona_id,
        ) {
            Ok((def, path, resolved_scope)) => {
                let raw = std::fs::read_to_string(&path).unwrap_or_default();
                ToolResult::success(
                    serde_json::to_string_pretty(&json!({
                        "definition": def,
                        "scope": resolved_scope,
                        "path": path.display().to_string(),
                        "yaml": raw,
                    }))
                    .unwrap_or_default(),
                )
            }
            Err(e) => ToolResult::error(e),
        }
    }
}

pub struct ValidateWorkflowTool {
    config: Arc<Config>,
}

impl ValidateWorkflowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for ValidateWorkflowTool {
    fn name(&self) -> &str {
        "validate_workflow"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "validate_workflow".into(),
            description: "Validate workflow YAML without writing to disk.".into(),
            input_schema: schema_object(
                json!({
                    "yaml": { "type": "string", "description": "Full workflow YAML document" }
                }),
                &["yaml"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        if !self.config.workflow_engine_enabled {
            return ToolResult::error("Workflow engine is disabled.".into());
        }
        let yaml = match input.get("yaml").and_then(|v| v.as_str()) {
            Some(y) => y,
            None => return ToolResult::error("Missing yaml".into()),
        };
        match parse_workflow_yaml(yaml) {
            Ok(def) => ToolResult::success(
                serde_json::to_string_pretty(&json!({ "valid": true, "id": def.id }))
                    .unwrap_or_default(),
            ),
            Err(e) => ToolResult::error(e),
        }
    }
}

pub struct WriteWorkflowTool {
    config: Arc<Config>,
}

impl WriteWorkflowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WriteWorkflowTool {
    fn name(&self) -> &str {
        "write_workflow"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_workflow".into(),
            description: "Create or replace an authored workflow YAML file. Activate create-workflow for new ids, modify-workflow for existing (enforced at runtime).".into(),
            input_schema: schema_object(
                json!({
                    "yaml": { "type": "string", "description": "Full workflow YAML document" },
                    "scope": { "type": "string", "description": "global or persona (default global)" }
                }),
                &["yaml"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        if !self.config.workflow_engine_enabled {
            return ToolResult::error("Workflow engine is disabled.".into());
        }
        let yaml = match input.get("yaml").and_then(|v| v.as_str()) {
            Some(y) => y,
            None => return ToolResult::error("Missing yaml".into()),
        };
        let auth = match auth_context_from_input(&input) {
            Some(a) => a,
            None => return ToolResult::error("Missing auth context".into()),
        };
        let def = match parse_workflow_yaml(yaml) {
            Ok(d) => d,
            Err(e) => return ToolResult::error(e),
        };
        let scope = scope_from_str(input.get("scope").and_then(|v| v.as_str()));
        let catalog = catalog_from_config(&self.config);
        match catalog.write(&def, scope, auth.caller_chat_id, auth.caller_persona_id) {
            Ok(path) => {
                ToolResult::success(format!("Wrote workflow '{}' to {}", def.id, path.display()))
            }
            Err(e) => ToolResult::error(e),
        }
    }
}

pub struct RunWorkflowTool {
    config: Arc<Config>,
    db: Arc<Database>,
    app_state: Arc<OnceLock<Arc<AppState>>>,
}

impl RunWorkflowTool {
    pub fn new(
        config: Arc<Config>,
        db: Arc<Database>,
        app_state: Arc<OnceLock<Arc<AppState>>>,
    ) -> Self {
        Self {
            config,
            db,
            app_state,
        }
    }
}

#[async_trait]
impl Tool for RunWorkflowTool {
    fn name(&self) -> &str {
        "run_workflow"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "run_workflow".into(),
            description: "Execute an authored workflow deterministically (fixed step order in YAML). Returns a JSON execution report.".into(),
            input_schema: schema_object(
                json!({
                    "workflow_id": { "type": "string", "description": "Workflow id" },
                    "inputs": { "type": "object", "description": "Optional input overrides" },
                    "scope": { "type": "string", "description": "global or persona" }
                }),
                &["workflow_id"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        if !self.config.workflow_engine_enabled {
            return ToolResult::error("Workflow engine is disabled.".into());
        }
        let workflow_id = match input.get("workflow_id").and_then(|v| v.as_str()) {
            Some(id) => id.trim(),
            None => return ToolResult::error("Missing workflow_id".into()),
        };
        let auth = match auth_context_from_input(&input) {
            Some(a) => a,
            None => return ToolResult::error("Missing auth context".into()),
        };
        let state = match self.app_state.get() {
            Some(s) => s.clone(),
            None => return ToolResult::error("App state not initialized".into()),
        };

        let scope = scope_from_str(input.get("scope").and_then(|v| v.as_str()));
        let catalog = catalog_from_config(&self.config);
        let (def, _path, _resolved) = match catalog.load(
            workflow_id,
            scope,
            auth.caller_chat_id,
            auth.caller_persona_id,
        ) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(e),
        };

        let defaults = default_inputs(&def.inputs);
        let overrides = input.get("inputs").cloned().unwrap_or(json!({}));
        let merged = merge_inputs(&defaults, &overrides);

        let run_key = input
            .get("run_key")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let report = execute_workflow(
            &def,
            merged,
            WorkflowExecutionParams {
                config: &state.config,
                db: self.db.clone(),
                tools: &state.tools,
                env_redactor: state.env_redactor.clone(),
                auth: &auth,
                chat_id: auth.caller_chat_id,
                persona_id: auth.caller_persona_id,
                caller_channel: &auth.caller_channel,
                is_scheduled_task: auth.is_scheduled_task,
                run_key: run_key.as_deref(),
                max_steps: self.config.workflow_max_steps,
            },
        )
        .await;

        report_to_tool_result(&report)
    }
}

pub fn register_workflow_tools(
    config: &Config,
    db: Arc<Database>,
    app_state_slot: Arc<OnceLock<Arc<AppState>>>,
    registry: &mut ToolRegistry,
) {
    if !config.workflow_engine_enabled {
        return;
    }
    let config = Arc::new(config.clone());
    registry.add_tool(Box::new(ListWorkflowsTool::new(config.clone())));
    registry.add_tool(Box::new(ReadWorkflowTool::new(config.clone())));
    registry.add_tool(Box::new(ValidateWorkflowTool::new(config.clone())));
    registry.add_tool(Box::new(WriteWorkflowTool::new(config.clone())));
    registry.add_tool(Box::new(RunWorkflowTool::new(config, db, app_state_slot)));
}
