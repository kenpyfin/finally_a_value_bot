//! Deterministic authored workflows (YAML on disk, Rust step executor).

pub mod catalog;
pub mod context;
pub mod executor;
pub mod schema;
pub mod template;

pub use catalog::WorkflowCatalog;
pub use executor::{execute_workflow, WorkflowExecutionParams};
pub use schema::{
    parse_workflow_yaml, validate_workflow_id, WorkflowDefinition, WorkflowListEntry,
    WorkflowRunReport, WorkflowScope,
};
