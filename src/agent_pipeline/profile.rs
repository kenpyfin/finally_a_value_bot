//! Hot-reloadable deterministic pipeline profile (Web UI).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::db::Database;
use crate::error::FinallyAValueBotError;
use crate::local_delegate::ModelTier;

pub const APP_SETTING_DETERMINISTIC_PIPELINE_CONFIG: &str = "DETERMINISTIC_PIPELINE_CONFIG";
pub const SCHEMA_VERSION: u32 = 6;
pub const MAX_PHASES: usize = 4;
pub const MAX_SYSTEM_PROMPT_CHARS: usize = 16_384;
pub const MAX_PREAMBLE_CHARS: usize = 4_096;
pub const MAX_PRIOR_STEP_SUMMARY_PROMPT_CHARS: usize = 4_096;
pub const DEFAULT_PRIOR_STEP_FULL_OUTPUT_MAX_CHARS: usize = 32_000;

// ---------------------------------------------------------------------------
// Phase kinds and routing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    IntentClassify,
    PlanGenerate,
    ExecutePlan,
    SynthesizeDelivery,
}

impl PhaseKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::IntentClassify => "intent_classify",
            Self::PlanGenerate => "plan_generate",
            Self::ExecutePlan => "execute_plan",
            Self::SynthesizeDelivery => "synthesize_delivery",
        }
    }

    pub fn all() -> &'static [PhaseKind] {
        &[
            Self::IntentClassify,
            Self::PlanGenerate,
            Self::ExecutePlan,
            Self::SynthesizeDelivery,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelRoute {
    #[default]
    InheritGlobal,
    Strategy,
    Local,
    /// Legacy stored profiles only; migrated to [`Local`]. Multi-model is single-tier today.
    Technical,
    /// Legacy stored profiles only; migrated to [`Local`]. Multi-model is single-tier today.
    Knowledge,
}

// ---------------------------------------------------------------------------
// Transitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionCondition {
    Always,
    IntentCategoryConversational,
    IntentCategoryQuestion,
    IntentCategoryTask,
    /// Ask user unless clarify-on-web/scheduler policy allows proceed.
    IntentNeedsClarification,
    /// Matched when clarification needed but channel policy proceeds on assumptions.
    IntentNeedsClarificationProceed,
    PlanEmpty,
    ExecuteAnyFailed,
    ExecuteAllSucceeded,
    ChannelWeb,
    IsScheduled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionTarget {
    DirectAnswer,
    Clarify,
    Finish,
    Phase(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionRule {
    pub when: TransitionCondition,
    pub goto: TransitionTarget,
}

// ---------------------------------------------------------------------------
// Layer 1 — Operational
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationalConfig {
    pub timeout_secs: u64,
    pub max_iterations: usize,
    pub max_iterations_local: usize,
    pub max_plan_steps: usize,
    pub llm_round_timeout_secs: u64,
    pub tool_execution_timeout_secs: u64,
    pub iteration_breaker_min_chars: usize,
    pub compact_system_max_chars: usize,
    pub collapsed_session_turns: usize,
    pub sop_reference_max_chars: usize,
    pub min_polish_only_summary_chars: usize,
    pub max_polish_only_combined_chars: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationalOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations_local: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_plan_steps: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_round_timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_execution_timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iteration_breaker_min_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_system_max_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapsed_session_turns: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sop_reference_max_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_polish_only_summary_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_polish_only_combined_chars: Option<usize>,
}

impl Default for OperationalConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 45,
            max_iterations: 3,
            max_iterations_local: 2,
            max_plan_steps: 4,
            llm_round_timeout_secs: 180,
            tool_execution_timeout_secs: 3600,
            iteration_breaker_min_chars: 300,
            compact_system_max_chars: 3500,
            collapsed_session_turns: 4,
            sop_reference_max_chars: 6000,
            min_polish_only_summary_chars: 80,
            max_polish_only_combined_chars: 2000,
        }
    }
}

// ---------------------------------------------------------------------------
// Layer 2 — Policy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyConfig {
    pub heuristic_intent_enabled: bool,
    pub merged_classify_and_plan_enabled: bool,
    pub skip_consolidate_when_good: bool,
    pub clarify_on_web_proceed: bool,
    pub clarify_on_scheduler_proceed: bool,
    pub image_input_force_task: bool,
    pub retry_failed_steps: bool,
    pub escalate_to_strategy_on_skill_failure: bool,
    pub use_local_for_json_stages: bool,
    /// When true, plan may attach persona Tier 2 SOPs from memory. Off by default — plans stay ephemeral unless intent names a vault SOP.
    pub bind_persona_sops_in_plan: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heuristic_intent_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_classify_and_plan_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_consolidate_when_good: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarify_on_web_proceed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarify_on_scheduler_proceed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_input_force_task: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_failed_steps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_to_strategy_on_skill_failure: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_local_for_json_stages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_persona_sops_in_plan: Option<bool>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            heuristic_intent_enabled: false,
            merged_classify_and_plan_enabled: true,
            skip_consolidate_when_good: true,
            clarify_on_web_proceed: true,
            clarify_on_scheduler_proceed: true,
            image_input_force_task: true,
            retry_failed_steps: true,
            escalate_to_strategy_on_skill_failure: true,
            use_local_for_json_stages: true,
            bind_persona_sops_in_plan: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-phase context inclusion (Layer 1.5)
// ---------------------------------------------------------------------------

pub(crate) const MINIMAL_SYSTEM_STUB: &str = "Respond according to the user message.";

pub(crate) const DEFAULT_PRIOR_STEP_SUMMARY_PROMPT: &str = "\
Summarize the prior step output for the next executor. \
Preserve exact file paths, URLs, command lines, exit codes, and error messages. \
Be concise but do not omit artifacts the next step must use.";

/// How prior execute-step output is passed to the next step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PriorStepFeedMode {
    /// Full step output (assistant text + tool I/O).
    #[default]
    Full,
    /// LLM summary using `prior_step_summary_prompt`.
    Summary,
}

/// Which context blocks are injected for a pipeline phase LLM call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhaseContextIncludes {
    /// Phase-specific system prompt (custom or builtin default for `kind`).
    #[serde(default = "default_true")]
    pub include_system_prompt: bool,
    /// Full agent system prompt from run prep (AGENTS.md, hooks, skills catalog, etc.).
    #[serde(default)]
    pub include_agent_system_prompt: bool,
    /// Skills catalog inside `pipeline_cloud_context`.
    #[serde(default = "default_true")]
    pub include_skills_catalog: bool,
    /// Recent conversation excerpt.
    #[serde(default = "default_true")]
    pub include_session_excerpt: bool,
    /// Persona memory (Tier 1 principles excerpt).
    #[serde(default = "default_true")]
    pub include_persona_memory: bool,
    /// Workspace paths and tool cwd.
    #[serde(default = "default_true")]
    pub include_workspace_paths: bool,
    /// Vault SOP reference body (plan / merged classify+plan).
    #[serde(default = "default_true")]
    pub include_sop_reference: bool,
    /// Current user request text.
    #[serde(default = "default_true")]
    pub include_current_request: bool,
    /// Prior step output fed to the next execute step (when `include_prior_step_summaries`).
    #[serde(default)]
    pub include_prior_step_summaries: bool,
    /// Full tool/assistant log vs LLM summary for prior-step handoff (execute only).
    #[serde(default)]
    pub prior_step_feed_mode: PriorStepFeedMode,
    /// System prompt for summary mode; empty uses builtin default.
    #[serde(default)]
    pub prior_step_summary_prompt: String,
    /// Cap on full output chars stored and forwarded per step.
    #[serde(default = "default_prior_step_full_max")]
    pub prior_step_full_output_max_chars: usize,
    /// Step contract fields in execute system/messages.
    #[serde(default = "default_true")]
    pub include_step_contract: bool,
    /// Intent goal + step results in consolidate user message.
    #[serde(default = "default_true")]
    pub include_execution_summary: bool,
}

fn default_true() -> bool {
    true
}

fn default_prior_step_full_max() -> usize {
    DEFAULT_PRIOR_STEP_FULL_OUTPUT_MAX_CHARS
}

impl PhaseContextIncludes {
    pub fn defaults_for_kind(kind: PhaseKind) -> Self {
        match kind {
            PhaseKind::IntentClassify => Self {
                include_system_prompt: true,
                include_agent_system_prompt: false,
                include_skills_catalog: true,
                include_session_excerpt: true,
                include_persona_memory: true,
                include_workspace_paths: true,
                include_sop_reference: false,
                include_current_request: true,
                include_prior_step_summaries: false,
                prior_step_feed_mode: PriorStepFeedMode::Full,
                prior_step_summary_prompt: String::new(),
                prior_step_full_output_max_chars: DEFAULT_PRIOR_STEP_FULL_OUTPUT_MAX_CHARS,
                include_step_contract: false,
                include_execution_summary: false,
            },
            PhaseKind::PlanGenerate => Self {
                include_system_prompt: true,
                include_agent_system_prompt: false,
                include_skills_catalog: true,
                include_session_excerpt: true,
                include_persona_memory: true,
                include_workspace_paths: true,
                include_sop_reference: false,
                include_current_request: true,
                include_prior_step_summaries: false,
                prior_step_feed_mode: PriorStepFeedMode::Full,
                prior_step_summary_prompt: String::new(),
                prior_step_full_output_max_chars: DEFAULT_PRIOR_STEP_FULL_OUTPUT_MAX_CHARS,
                include_step_contract: false,
                include_execution_summary: false,
            },
            PhaseKind::ExecutePlan => Self {
                include_system_prompt: true,
                include_agent_system_prompt: false,
                include_skills_catalog: false,
                include_session_excerpt: false,
                include_persona_memory: false,
                include_workspace_paths: false,
                include_sop_reference: false,
                include_current_request: true,
                include_prior_step_summaries: true,
                prior_step_feed_mode: PriorStepFeedMode::Full,
                prior_step_summary_prompt: String::new(),
                prior_step_full_output_max_chars: DEFAULT_PRIOR_STEP_FULL_OUTPUT_MAX_CHARS,
                include_step_contract: true,
                include_execution_summary: false,
            },
            PhaseKind::SynthesizeDelivery => Self {
                include_system_prompt: true,
                include_agent_system_prompt: false,
                include_skills_catalog: false,
                include_session_excerpt: false,
                include_persona_memory: false,
                include_workspace_paths: false,
                include_sop_reference: false,
                include_current_request: true,
                include_prior_step_summaries: false,
                prior_step_feed_mode: PriorStepFeedMode::Full,
                prior_step_summary_prompt: String::new(),
                prior_step_full_output_max_chars: DEFAULT_PRIOR_STEP_FULL_OUTPUT_MAX_CHARS,
                include_step_contract: false,
                include_execution_summary: true,
            },
        }
    }
}

impl Default for PhaseContextIncludes {
    fn default() -> Self {
        Self::defaults_for_kind(PhaseKind::IntentClassify)
    }
}

// ---------------------------------------------------------------------------
// Phase + profile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelinePhase {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub kind: PhaseKind,
    pub model_route: ModelRoute,
    /// Empty string uses the builtin default for `kind`.
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preamble: Option<String>,
    #[serde(default)]
    pub context_includes: PhaseContextIncludes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub operational: OperationalOverrides,
    #[serde(default)]
    pub policies: PolicyOverrides,
    #[serde(default)]
    pub transitions: Vec<TransitionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineProfile {
    pub version: u32,
    pub entry_phase_id: String,
    pub phases: Vec<PipelinePhase>,
    pub operational: OperationalConfig,
    pub policies: PolicyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Resolved helpers (runtime)
// ---------------------------------------------------------------------------

pub struct ResolvedPhase<'a> {
    pub phase: &'a PipelinePhase,
    pub operational: OperationalConfig,
    pub policies: PolicyConfig,
}

impl PipelineProfile {
    pub fn default_profile() -> Self {
        Self {
            version: SCHEMA_VERSION,
            entry_phase_id: "intent".into(),
            operational: OperationalConfig::default(),
            policies: PolicyConfig::default(),
            phases: vec![
                PipelinePhase {
                    id: "intent".into(),
                    label: "Intent".into(),
                    enabled: true,
                    kind: PhaseKind::IntentClassify,
                    model_route: ModelRoute::InheritGlobal,
                    system_prompt: String::new(),
                    preamble: None,
                    context_includes: PhaseContextIncludes::defaults_for_kind(
                        PhaseKind::IntentClassify,
                    ),
                    allowed_tools: None,
                    operational: OperationalOverrides::default(),
                    policies: PolicyOverrides::default(),
                    transitions: default_intent_phase_transitions(),
                },
                PipelinePhase {
                    id: "plan".into(),
                    label: "Plan".into(),
                    enabled: true,
                    kind: PhaseKind::PlanGenerate,
                    model_route: ModelRoute::InheritGlobal,
                    system_prompt: String::new(),
                    preamble: None,
                    context_includes: PhaseContextIncludes::defaults_for_kind(
                        PhaseKind::PlanGenerate,
                    ),
                    allowed_tools: None,
                    operational: OperationalOverrides::default(),
                    policies: PolicyOverrides::default(),
                    transitions: vec![TransitionRule {
                        when: TransitionCondition::Always,
                        goto: TransitionTarget::Phase("execute".into()),
                    }],
                },
                PipelinePhase {
                    id: "execute".into(),
                    label: "Execute".into(),
                    enabled: true,
                    kind: PhaseKind::ExecutePlan,
                    model_route: ModelRoute::Local,
                    system_prompt: String::new(),
                    preamble: None,
                    context_includes: PhaseContextIncludes::defaults_for_kind(
                        PhaseKind::ExecutePlan,
                    ),
                    allowed_tools: None,
                    operational: OperationalOverrides::default(),
                    policies: PolicyOverrides::default(),
                    transitions: vec![TransitionRule {
                        when: TransitionCondition::Always,
                        goto: TransitionTarget::Phase("consolidate".into()),
                    }],
                },
                PipelinePhase {
                    id: "consolidate".into(),
                    label: "Consolidate".into(),
                    enabled: true,
                    kind: PhaseKind::SynthesizeDelivery,
                    model_route: ModelRoute::Strategy,
                    system_prompt: String::new(),
                    preamble: None,
                    context_includes: PhaseContextIncludes::defaults_for_kind(
                        PhaseKind::SynthesizeDelivery,
                    ),
                    allowed_tools: None,
                    operational: OperationalOverrides::default(),
                    policies: PolicyOverrides::default(),
                    transitions: vec![TransitionRule {
                        when: TransitionCondition::Always,
                        goto: TransitionTarget::Finish,
                    }],
                },
            ],
        }
    }

    pub fn migrate(mut self) -> Self {
        if self.version >= SCHEMA_VERSION {
            return self;
        }
        if self.version < 2 {
            for phase in &mut self.phases {
                phase.context_includes = PhaseContextIncludes::defaults_for_kind(phase.kind);
            }
        }
        if self.version < 3 {
            for phase in &mut self.phases {
                let defaults = PhaseContextIncludes::defaults_for_kind(phase.kind);
                phase.context_includes.prior_step_feed_mode = defaults.prior_step_feed_mode;
                phase.context_includes.prior_step_summary_prompt =
                    defaults.prior_step_summary_prompt.clone();
                phase.context_includes.prior_step_full_output_max_chars =
                    defaults.prior_step_full_output_max_chars;
            }
        }
        if self.version < 4 {
            for phase in &mut self.phases {
                if phase.kind == PhaseKind::IntentClassify {
                    phase.transitions = default_intent_phase_transitions();
                }
            }
            self.policies.heuristic_intent_enabled = false;
        }
        if self.version < 5 {
            for phase in &mut self.phases {
                if phase.kind == PhaseKind::PlanGenerate {
                    phase.context_includes.include_sop_reference = false;
                }
            }
            self.policies.bind_persona_sops_in_plan = false;
        }
        if self.version < 6 {
            for phase in &mut self.phases {
                if matches!(
                    phase.model_route,
                    ModelRoute::Technical | ModelRoute::Knowledge
                ) {
                    phase.model_route = ModelRoute::Local;
                }
            }
        }
        self.version = SCHEMA_VERSION;
        self
    }

    pub fn phase_by_id(&self, id: &str) -> Option<&PipelinePhase> {
        self.phases.iter().find(|p| p.id == id)
    }

    pub fn resolve_phase<'a>(&'a self, phase: &'a PipelinePhase) -> ResolvedPhase<'a> {
        ResolvedPhase {
            operational: merge_operational(&self.operational, &phase.operational),
            policies: merge_policies(&self.policies, &phase.policies),
            phase,
        }
    }

    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        if self.version != SCHEMA_VERSION {
            errors.push(ValidationError {
                path: "version".into(),
                message: format!("unsupported schema version {}", self.version),
            });
        }

        if self.phases.is_empty() || self.phases.len() > MAX_PHASES {
            errors.push(ValidationError {
                path: "phases".into(),
                message: format!("must have 1..={MAX_PHASES} phases"),
            });
        }

        let mut ids = HashSet::new();
        for (i, phase) in self.phases.iter().enumerate() {
            let base = format!("phases[{i}]");
            if phase.id.trim().is_empty() {
                errors.push(ValidationError {
                    path: format!("{base}.id"),
                    message: "id is required".into(),
                });
            } else if !ids.insert(phase.id.clone()) {
                errors.push(ValidationError {
                    path: format!("{base}.id"),
                    message: "duplicate phase id".into(),
                });
            }
            if phase.system_prompt.chars().count() > MAX_SYSTEM_PROMPT_CHARS {
                errors.push(ValidationError {
                    path: format!("{base}.system_prompt"),
                    message: format!("max {MAX_SYSTEM_PROMPT_CHARS} characters"),
                });
            }
            if let Some(ref preamble) = phase.preamble {
                if preamble.chars().count() > MAX_PREAMBLE_CHARS {
                    errors.push(ValidationError {
                        path: format!("{base}.preamble"),
                        message: format!("max {MAX_PREAMBLE_CHARS} characters"),
                    });
                }
            }
            let ctx_path = format!("{base}.context_includes");
            if phase
                .context_includes
                .prior_step_summary_prompt
                .chars()
                .count()
                > MAX_PRIOR_STEP_SUMMARY_PROMPT_CHARS
            {
                errors.push(ValidationError {
                    path: format!("{ctx_path}.prior_step_summary_prompt"),
                    message: format!("max {MAX_PRIOR_STEP_SUMMARY_PROMPT_CHARS} characters"),
                });
            }
            let full_max = phase.context_includes.prior_step_full_output_max_chars;
            if !(1_000..=200_000).contains(&full_max) {
                errors.push(ValidationError {
                    path: format!("{ctx_path}.prior_step_full_output_max_chars"),
                    message: "must be between 1000 and 200000".into(),
                });
            }
            validate_operational(
                &phase.operational,
                &format!("{base}.operational"),
                &mut errors,
            );
        }

        if self.entry_phase_id.trim().is_empty() {
            errors.push(ValidationError {
                path: "entry_phase_id".into(),
                message: "entry phase is required".into(),
            });
        } else if let Some(entry) = self.phase_by_id(&self.entry_phase_id) {
            if !entry.enabled {
                errors.push(ValidationError {
                    path: "entry_phase_id".into(),
                    message: "entry phase must be enabled".into(),
                });
            }
        } else {
            errors.push(ValidationError {
                path: "entry_phase_id".into(),
                message: "unknown phase id".into(),
            });
        }

        validate_operational_global(&self.operational, &mut errors);

        let phase_ids: HashSet<_> = self.phases.iter().map(|p| p.id.as_str()).collect();
        for (i, phase) in self.phases.iter().enumerate() {
            if !phase.enabled {
                continue;
            }
            for (j, rule) in phase.transitions.iter().enumerate() {
                validate_transition_target(
                    &rule.goto,
                    &phase_ids,
                    &format!("phases[{i}].transitions[{j}].goto"),
                    &mut errors,
                );
            }
        }

        for (i, phase) in self.phases.iter().enumerate() {
            if !phase.enabled {
                if phase.id == self.entry_phase_id {
                    errors.push(ValidationError {
                        path: format!("phases[{i}].enabled"),
                        message: "entry phase cannot be disabled".into(),
                    });
                }
                for other in &self.phases {
                    for rule in &other.transitions {
                        if matches!(
                            &rule.goto,
                            TransitionTarget::Phase(id) if id == &phase.id
                        ) {
                            errors.push(ValidationError {
                                path: format!("phases[{i}].enabled"),
                                message: "disabled phase is referenced by a transition".into(),
                            });
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            if let Err(e) = validate_graph_terminates(self) {
                errors.push(ValidationError {
                    path: "phases".into(),
                    message: e,
                });
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn validate_operational_global(op: &OperationalConfig, errors: &mut Vec<ValidationError>) {
    validate_operational_field("operational.timeout_secs", op.timeout_secs, 5, 600, errors);
    validate_operational_field_usize(
        "operational.max_iterations",
        op.max_iterations,
        1,
        10,
        errors,
    );
    validate_operational_field_usize(
        "operational.max_iterations_local",
        op.max_iterations_local,
        1,
        10,
        errors,
    );
    validate_operational_field_usize(
        "operational.max_plan_steps",
        op.max_plan_steps,
        1,
        8,
        errors,
    );
    validate_operational_field(
        "operational.llm_round_timeout_secs",
        op.llm_round_timeout_secs,
        10,
        600,
        errors,
    );
    validate_operational_field(
        "operational.tool_execution_timeout_secs",
        op.tool_execution_timeout_secs,
        30,
        7200,
        errors,
    );
}

fn validate_operational(ov: &OperationalOverrides, base: &str, errors: &mut Vec<ValidationError>) {
    if let Some(v) = ov.timeout_secs {
        validate_operational_field(&format!("{base}.timeout_secs"), v, 5, 600, errors);
    }
    if let Some(v) = ov.max_iterations {
        validate_operational_field_usize(&format!("{base}.max_iterations"), v, 1, 10, errors);
    }
    if let Some(v) = ov.max_iterations_local {
        validate_operational_field_usize(&format!("{base}.max_iterations_local"), v, 1, 10, errors);
    }
    if let Some(v) = ov.max_plan_steps {
        validate_operational_field_usize(&format!("{base}.max_plan_steps"), v, 1, 8, errors);
    }
}

fn validate_operational_field(
    path: &str,
    v: u64,
    min: u64,
    max: u64,
    errors: &mut Vec<ValidationError>,
) {
    if v < min || v > max {
        errors.push(ValidationError {
            path: path.into(),
            message: format!("must be between {min} and {max}"),
        });
    }
}

fn validate_operational_field_usize(
    path: &str,
    v: usize,
    min: usize,
    max: usize,
    errors: &mut Vec<ValidationError>,
) {
    if v < min || v > max {
        errors.push(ValidationError {
            path: path.into(),
            message: format!("must be between {min} and {max}"),
        });
    }
}

fn validate_transition_target(
    target: &TransitionTarget,
    phase_ids: &HashSet<&str>,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    if let TransitionTarget::Phase(id) = target {
        if !phase_ids.contains(id.as_str()) {
            errors.push(ValidationError {
                path: path.into(),
                message: format!("unknown phase id '{id}'"),
            });
        }
    }
}

fn default_intent_phase_transitions() -> Vec<TransitionRule> {
    vec![TransitionRule {
        when: TransitionCondition::Always,
        goto: TransitionTarget::Phase("plan".into()),
    }]
}

fn validate_graph_terminates(profile: &PipelineProfile) -> Result<(), String> {
    let mut visits: HashMap<String, u32> = HashMap::new();
    let mut current = profile.entry_phase_id.clone();
    let max_steps = 64usize;

    for _ in 0..max_steps {
        let Some(phase) = profile.phase_by_id(&current) else {
            return Err(format!("unreachable phase '{current}'"));
        };
        if !phase.enabled {
            return Err(format!("disabled phase '{current}' in path"));
        }
        let count = visits.entry(current.clone()).or_insert(0);
        *count += 1;
        if *count > 8 {
            return Err(format!("possible loop at phase '{current}'"));
        }

        let ctx = TransitionEvalContext {
            intent: None,
            plan_empty: false,
            execute_any_failed: false,
            execute_all_succeeded: false,
            caller_channel: "",
            is_scheduled_task: false,
            is_background_job: false,
        };
        let target = resolve_transition(phase, &ctx, &profile.policies)
            .ok_or_else(|| format!("no matching transition in phase '{}'", phase.id))?;

        match target {
            TransitionTarget::Finish
            | TransitionTarget::DirectAnswer
            | TransitionTarget::Clarify => return Ok(()),
            TransitionTarget::Phase(next) => current = next,
        }
    }
    Err("graph validation exceeded step budget".into())
}

pub struct TransitionEvalContext<'a> {
    pub intent: Option<&'a super::intent::IntentDecision>,
    pub plan_empty: bool,
    pub execute_any_failed: bool,
    pub execute_all_succeeded: bool,
    pub caller_channel: &'a str,
    pub is_scheduled_task: bool,
    pub is_background_job: bool,
}

pub fn should_proceed_despite_clarify(
    ctx: &TransitionEvalContext<'_>,
    policies: &PolicyConfig,
) -> bool {
    if ctx.is_scheduled_task || ctx.is_background_job {
        return policies.clarify_on_scheduler_proceed;
    }
    if ctx.caller_channel == "web" {
        return policies.clarify_on_web_proceed;
    }
    false
}

pub fn resolve_transition(
    phase: &PipelinePhase,
    ctx: &TransitionEvalContext<'_>,
    policies: &PolicyConfig,
) -> Option<TransitionTarget> {
    for rule in &phase.transitions {
        if matches_condition(&rule.when, ctx, policies) {
            return Some(rule.goto.clone());
        }
    }
    None
}

fn matches_condition(
    cond: &TransitionCondition,
    ctx: &TransitionEvalContext<'_>,
    policies: &PolicyConfig,
) -> bool {
    use super::intent::IntentCategory;
    match cond {
        TransitionCondition::Always => true,
        TransitionCondition::IntentCategoryConversational => ctx
            .intent
            .is_some_and(|i| i.category == IntentCategory::Conversational),
        TransitionCondition::IntentCategoryQuestion => ctx
            .intent
            .is_some_and(|i| i.category == IntentCategory::Question),
        TransitionCondition::IntentCategoryTask => ctx
            .intent
            .is_some_and(|i| i.category == IntentCategory::Task),
        TransitionCondition::IntentNeedsClarification => ctx.intent.is_some_and(|i| {
            i.needs_clarification && !should_proceed_despite_clarify(ctx, policies)
        }),
        TransitionCondition::IntentNeedsClarificationProceed => ctx.intent.is_some_and(|i| {
            i.needs_clarification && should_proceed_despite_clarify(ctx, policies)
        }),
        TransitionCondition::PlanEmpty => ctx.plan_empty,
        TransitionCondition::ExecuteAnyFailed => ctx.execute_any_failed,
        TransitionCondition::ExecuteAllSucceeded => ctx.execute_all_succeeded,
        TransitionCondition::ChannelWeb => ctx.caller_channel == "web",
        TransitionCondition::IsScheduled => ctx.is_scheduled_task || ctx.is_background_job,
    }
}

pub fn merge_operational(
    global: &OperationalConfig,
    ov: &OperationalOverrides,
) -> OperationalConfig {
    OperationalConfig {
        timeout_secs: ov.timeout_secs.unwrap_or(global.timeout_secs),
        max_iterations: ov.max_iterations.unwrap_or(global.max_iterations),
        max_iterations_local: ov
            .max_iterations_local
            .unwrap_or(global.max_iterations_local),
        max_plan_steps: ov.max_plan_steps.unwrap_or(global.max_plan_steps),
        llm_round_timeout_secs: ov
            .llm_round_timeout_secs
            .unwrap_or(global.llm_round_timeout_secs),
        tool_execution_timeout_secs: ov
            .tool_execution_timeout_secs
            .unwrap_or(global.tool_execution_timeout_secs),
        iteration_breaker_min_chars: ov
            .iteration_breaker_min_chars
            .unwrap_or(global.iteration_breaker_min_chars),
        compact_system_max_chars: ov
            .compact_system_max_chars
            .unwrap_or(global.compact_system_max_chars),
        collapsed_session_turns: ov
            .collapsed_session_turns
            .unwrap_or(global.collapsed_session_turns),
        sop_reference_max_chars: ov
            .sop_reference_max_chars
            .unwrap_or(global.sop_reference_max_chars),
        min_polish_only_summary_chars: ov
            .min_polish_only_summary_chars
            .unwrap_or(global.min_polish_only_summary_chars),
        max_polish_only_combined_chars: ov
            .max_polish_only_combined_chars
            .unwrap_or(global.max_polish_only_combined_chars),
    }
}

pub fn merge_policies(global: &PolicyConfig, ov: &PolicyOverrides) -> PolicyConfig {
    PolicyConfig {
        heuristic_intent_enabled: ov
            .heuristic_intent_enabled
            .unwrap_or(global.heuristic_intent_enabled),
        merged_classify_and_plan_enabled: ov
            .merged_classify_and_plan_enabled
            .unwrap_or(global.merged_classify_and_plan_enabled),
        skip_consolidate_when_good: ov
            .skip_consolidate_when_good
            .unwrap_or(global.skip_consolidate_when_good),
        clarify_on_web_proceed: ov
            .clarify_on_web_proceed
            .unwrap_or(global.clarify_on_web_proceed),
        clarify_on_scheduler_proceed: ov
            .clarify_on_scheduler_proceed
            .unwrap_or(global.clarify_on_scheduler_proceed),
        image_input_force_task: ov
            .image_input_force_task
            .unwrap_or(global.image_input_force_task),
        retry_failed_steps: ov.retry_failed_steps.unwrap_or(global.retry_failed_steps),
        escalate_to_strategy_on_skill_failure: ov
            .escalate_to_strategy_on_skill_failure
            .unwrap_or(global.escalate_to_strategy_on_skill_failure),
        use_local_for_json_stages: ov
            .use_local_for_json_stages
            .unwrap_or(global.use_local_for_json_stages),
        bind_persona_sops_in_plan: ov
            .bind_persona_sops_in_plan
            .unwrap_or(global.bind_persona_sops_in_plan),
    }
}

pub fn resolve_model_tier(
    route: ModelRoute,
    state: &crate::telegram::AppState,
    policies: &PolicyConfig,
) -> ModelTier {
    let mm = state.llm.local_delegate_config();
    match route {
        ModelRoute::Strategy => ModelTier::Strategy,
        ModelRoute::Local | ModelRoute::Technical | ModelRoute::Knowledge => {
            if mm.local_routable() {
                ModelTier::LocalReadOnly
            } else {
                ModelTier::Strategy
            }
        }
        ModelRoute::InheritGlobal => {
            if policies.use_local_for_json_stages && mm.local_routable() {
                ModelTier::LocalReadOnly
            } else {
                ModelTier::Strategy
            }
        }
    }
}

pub fn builtin_prompt_for_kind(kind: PhaseKind) -> &'static str {
    match kind {
        PhaseKind::IntentClassify => super::intent::builtin_intent_system_prompt(),
        PhaseKind::PlanGenerate => super::plan::builtin_plan_system_prompt(),
        PhaseKind::ExecutePlan => super::execute::builtin_step_execute_preamble(),
        PhaseKind::SynthesizeDelivery => super::consolidate::builtin_delivery_system_prompt(),
    }
}

pub fn effective_system_prompt(phase: &PipelinePhase, kind: PhaseKind) -> String {
    if !phase.system_prompt.trim().is_empty() {
        return phase.system_prompt.clone();
    }
    builtin_prompt_for_kind(kind).to_string()
}

/// Compose the system prompt sent to the LLM for a pipeline phase.
pub fn compose_system_prompt(
    phase: &PipelinePhase,
    kind: PhaseKind,
    agent_system_prompt: Option<&str>,
    includes: &PhaseContextIncludes,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if includes.include_agent_system_prompt {
        if let Some(p) = agent_system_prompt.filter(|s| !s.trim().is_empty()) {
            parts.push(p.to_string());
        }
    }
    if includes.include_system_prompt {
        parts.push(effective_system_prompt(phase, kind));
    }
    if parts.is_empty() {
        MINIMAL_SYSTEM_STUB.to_string()
    } else {
        parts.join("\n\n---\n\n")
    }
}

pub fn load_from_db(db: &Database) -> Result<PipelineProfile, FinallyAValueBotError> {
    let rows: Vec<(String, String)> = db
        .list_app_settings()?
        .into_iter()
        .map(|s| (s.key, s.value))
        .collect();
    let raw = rows
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(APP_SETTING_DETERMINISTIC_PIPELINE_CONFIG))
        .map(|(_, v)| v.as_str());
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return Ok(PipelineProfile::default_profile());
    };
    let profile: PipelineProfile = serde_json::from_str(raw)
        .map_err(|e| FinallyAValueBotError::Config(format!("pipeline profile JSON parse: {e}")))?;
    let profile = profile.migrate();
    profile.validate().map_err(|errs| {
        FinallyAValueBotError::Config(format!(
            "stored pipeline profile invalid: {}",
            errs.iter()
                .map(|e| format!("{}: {}", e.path, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        ))
    })?;
    Ok(profile)
}

pub fn persist_to_db(
    db: &Database,
    profile: &PipelineProfile,
) -> Result<(), FinallyAValueBotError> {
    profile.validate().map_err(|errs| {
        FinallyAValueBotError::Config(format!(
            "pipeline profile validation failed: {}",
            errs.iter()
                .map(|e| format!("{}: {}", e.path, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        ))
    })?;
    let json = serde_json::to_string(profile)
        .map_err(|e| FinallyAValueBotError::Config(format!("pipeline profile serialize: {e}")))?;
    db.set_app_setting(APP_SETTING_DETERMINISTIC_PIPELINE_CONFIG, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_validates() {
        let p = PipelineProfile::default_profile();
        p.validate().expect("default profile valid");
    }

    #[test]
    fn rejects_too_many_phases() {
        let mut p = PipelineProfile::default_profile();
        p.phases.push(PipelinePhase {
            id: "extra".into(),
            label: "Extra".into(),
            enabled: true,
            kind: PhaseKind::PlanGenerate,
            model_route: ModelRoute::Strategy,
            system_prompt: String::new(),
            preamble: None,
            context_includes: PhaseContextIncludes::defaults_for_kind(PhaseKind::PlanGenerate),
            allowed_tools: None,
            operational: OperationalOverrides::default(),
            policies: PolicyOverrides::default(),
            transitions: vec![TransitionRule {
                when: TransitionCondition::Always,
                goto: TransitionTarget::Finish,
            }],
        });
        assert!(p.validate().is_err());
    }

    #[test]
    fn migrate_v6_maps_legacy_model_routes_to_local() {
        let mut p = PipelineProfile::default_profile();
        p.version = 5;
        p.phases[2].model_route = ModelRoute::Technical;
        p.phases[3].model_route = ModelRoute::Knowledge;
        let migrated = p.migrate();
        assert_eq!(migrated.version, SCHEMA_VERSION);
        assert_eq!(migrated.phases[2].model_route, ModelRoute::Local);
        assert_eq!(migrated.phases[3].model_route, ModelRoute::Local);
    }

    #[test]
    fn migrate_v1_profile_adds_context_defaults() {
        let mut p = PipelineProfile::default_profile();
        p.version = 1;
        p.phases[2].context_includes.include_skills_catalog = true;
        let migrated = p.migrate();
        assert_eq!(migrated.version, SCHEMA_VERSION);
        assert!(!migrated.phases[2].context_includes.include_skills_catalog);
    }
}
