use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::schema::{
    parse_workflow_yaml, validate_workflow_id, WorkflowDefinition, WorkflowListEntry, WorkflowScope,
};

#[derive(Debug, Clone)]
pub struct WorkflowCatalog {
    workspace_dir: PathBuf,
    definitions_dir: String,
    allow_persona_scope: bool,
}

impl WorkflowCatalog {
    pub fn new(
        workspace_dir: impl AsRef<Path>,
        definitions_dir: &str,
        allow_persona_scope: bool,
    ) -> Self {
        Self {
            workspace_dir: workspace_dir.as_ref().to_path_buf(),
            definitions_dir: definitions_dir.to_string(),
            allow_persona_scope,
        }
    }

    pub fn global_workflows_dir(&self) -> PathBuf {
        self.workspace_dir.join(&self.definitions_dir)
    }

    pub fn persona_workflows_dir(&self, chat_id: i64, persona_id: i64) -> PathBuf {
        self.workspace_dir
            .join("shared")
            .join("personas")
            .join(chat_id.to_string())
            .join(persona_id.to_string())
            .join("workflows")
    }

    pub fn workflow_file_path(
        &self,
        scope: WorkflowScope,
        chat_id: i64,
        persona_id: i64,
        workflow_id: &str,
    ) -> Result<PathBuf, String> {
        validate_workflow_id(workflow_id)?;
        let dir = match scope {
            WorkflowScope::Global => self.global_workflows_dir(),
            WorkflowScope::Persona => {
                if !self.allow_persona_scope {
                    return Err("persona-scoped workflows are disabled".into());
                }
                self.persona_workflows_dir(chat_id, persona_id)
            }
        };
        Ok(dir.join(format!("{workflow_id}.workflow.yaml")))
    }

    fn collect_yaml_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if !dir.is_dir() {
            return files;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return files;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e == "yaml" || e == "yml")
            {
                files.push(path);
            }
        }
        files.sort();
        files
    }

    pub fn list_entries(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Vec<WorkflowListEntry>, String> {
        let mut by_id: HashMap<String, WorkflowListEntry> = HashMap::new();

        for path in Self::collect_yaml_files(&self.global_workflows_dir()) {
            if let Ok(entry) = self.entry_from_path(path, WorkflowScope::Global) {
                by_id.insert(entry.id.clone(), entry);
            }
        }

        if self.allow_persona_scope {
            for path in Self::collect_yaml_files(&self.persona_workflows_dir(chat_id, persona_id)) {
                if let Ok(entry) = self.entry_from_path(path, WorkflowScope::Persona) {
                    by_id.insert(entry.id.clone(), entry);
                }
            }
        }

        let mut entries: Vec<_> = by_id.into_values().collect();
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(entries)
    }

    fn entry_from_path(
        &self,
        path: PathBuf,
        scope: WorkflowScope,
    ) -> Result<WorkflowListEntry, String> {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let def = parse_workflow_yaml(&raw)?;
        Ok(WorkflowListEntry {
            id: def.id.clone(),
            description: def.description.clone(),
            enabled: def.enabled,
            version: def.version,
            step_count: def.steps.len(),
            scope,
            path: path.display().to_string(),
        })
    }

    pub fn load(
        &self,
        workflow_id: &str,
        scope: WorkflowScope,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<(WorkflowDefinition, PathBuf, WorkflowScope), String> {
        validate_workflow_id(workflow_id)?;

        let persona_path =
            self.workflow_file_path(WorkflowScope::Persona, chat_id, persona_id, workflow_id)?;
        let global_path =
            self.workflow_file_path(WorkflowScope::Global, chat_id, persona_id, workflow_id)?;

        let (path, resolved_scope) = match scope {
            WorkflowScope::Persona => {
                if persona_path.is_file() {
                    (persona_path, WorkflowScope::Persona)
                } else if global_path.is_file() {
                    (global_path, WorkflowScope::Global)
                } else {
                    return Err(format!("workflow '{workflow_id}' not found"));
                }
            }
            WorkflowScope::Global => {
                if global_path.is_file() {
                    (global_path, WorkflowScope::Global)
                } else if self.allow_persona_scope && persona_path.is_file() {
                    (persona_path, WorkflowScope::Persona)
                } else {
                    return Err(format!("workflow '{workflow_id}' not found"));
                }
            }
        };

        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let def = parse_workflow_yaml(&raw)?;
        if def.id != workflow_id {
            return Err(format!(
                "workflow file id '{}' does not match requested id '{workflow_id}'",
                def.id
            ));
        }
        Ok((def, path, resolved_scope))
    }

    pub fn write(
        &self,
        def: &WorkflowDefinition,
        scope: WorkflowScope,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<PathBuf, String> {
        validate_definition(def)?;
        let path = self.workflow_file_path(scope, chat_id, persona_id, &def.id)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        let yaml =
            serde_yaml::to_string(def).map_err(|e| format!("failed to serialize YAML: {e}"))?;
        std::fs::write(&path, yaml)
            .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
        Ok(path)
    }

    pub fn exists(&self, workflow_id: &str, chat_id: i64, persona_id: i64) -> bool {
        self.workflow_file_path(WorkflowScope::Global, chat_id, persona_id, workflow_id)
            .ok()
            .map(|p| p.is_file())
            .unwrap_or(false)
            || (self.allow_persona_scope
                && self
                    .workflow_file_path(WorkflowScope::Persona, chat_id, persona_id, workflow_id)
                    .ok()
                    .map(|p| p.is_file())
                    .unwrap_or(false))
    }
}

fn validate_definition(def: &WorkflowDefinition) -> Result<(), String> {
    super::schema::validate_definition(def)
}
