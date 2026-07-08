//! Rich context bundle for cloud (strategy) intent/plan stages.
//!
//! Local execute should receive only the step contract produced here — not replay full chat history.

use std::collections::HashSet;

use crate::claude::{ContentBlock, Message, MessageContent};
use crate::db::call_blocking;
use crate::skills::{SkillManager, SkillsCatalogMode};
use crate::telegram::AgentRunPrep;
use crate::telegram::AppState;

use super::profile::PhaseContextIncludes;

/// Default cap for conversation excerpt injected into intent/plan LLM calls.
pub const DEFAULT_CLOUD_SESSION_EXCERPT_MAX_CHARS: usize = 16_000;

/// Per-message cap when building the session excerpt (cloud stages only).
const CLOUD_MESSAGE_MAX_CHARS: usize = 2_000;

#[derive(Debug, Clone)]
pub struct PipelineCloudContext {
    /// Skill names the persona may use (for plan validation).
    pub known_skill_names: HashSet<String>,
    skills_catalog: String,
    session_excerpt: String,
    memory_block: String,
    workspace_block: String,
}

impl PipelineCloudContext {
    pub fn append_to_user_message(&self, body: &str, includes: &PhaseContextIncludes) -> String {
        let formatted = self.format_for(includes);
        if formatted.trim().is_empty() {
            body.to_string()
        } else {
            format!("{}\n\n{}", formatted.trim_end(), body)
        }
    }

    pub fn format_for(&self, includes: &PhaseContextIncludes) -> String {
        let show_skills = includes.include_skills_catalog && !self.skills_catalog.trim().is_empty();
        let show_session =
            includes.include_session_excerpt && !self.session_excerpt.trim().is_empty();
        let show_memory = includes.include_persona_memory && !self.memory_block.trim().is_empty();
        let show_workspace =
            includes.include_workspace_paths && !self.workspace_block.trim().is_empty();

        if !show_skills && !show_session && !show_memory && !show_workspace {
            return String::new();
        }

        let mut sections = String::from(
            "[pipeline_cloud_context]\n\
             Cloud stage: use this context to classify intent and produce an executable plan.\n\
             Local execute will receive ONLY your plan step contracts — be explicit (skill_name, skill_script, skill_args_hint, paths).\n\
             skill_name MUST be an exact id from the skills catalog below (never invent names).\n\n",
        );

        if show_workspace {
            sections.push_str(&self.workspace_block);
            sections.push('\n');
        }
        if show_memory {
            sections.push_str(&self.memory_block);
        }
        if show_skills {
            sections.push_str("## Skills catalog\n");
            sections.push_str(&self.skills_catalog);
            sections.push_str("\n\n");
        }
        if show_session {
            sections.push_str("## Recent conversation\n");
            sections.push_str(&self.session_excerpt);
            sections.push('\n');
        }
        sections.push_str("[/pipeline_cloud_context]");
        sections
    }
}

pub async fn build_pipeline_cloud_context(
    state: &AppState,
    prep: &AgentRunPrep,
    session_max_chars: usize,
    skills_mode: SkillsCatalogMode,
) -> PipelineCloudContext {
    let chat_id = prep.tool_auth.caller_chat_id;
    let persona_id = prep.tool_auth.caller_persona_id;

    let allowed_skill_names = call_blocking(state.db.clone(), move |db| {
        Ok(db
            .get_persona_hook_skill_policy(chat_id, persona_id)?
            .and_then(|p| p.allowed_skill_names)
            .map(|v| v.into_iter().collect::<HashSet<String>>()))
    })
    .await
    .ok()
    .flatten();

    let manager = SkillManager::from_skills_dirs(state.config.skill_discovery_dirs());
    let known_skill_names: HashSet<String> = manager
        .discover_skills_for_allowed(allowed_skill_names.as_ref())
        .into_iter()
        .map(|s| s.name)
        .collect();

    let skills_catalog = manager
        .build_skills_catalog_for_allowed_with_mode(skills_mode, allowed_skill_names.as_ref());

    let workspace = crate::tools::persona_shared_dir(
        std::path::Path::new(state.config.working_dir()),
        chat_id,
        persona_id,
    );
    let workspace_display = workspace.to_string_lossy();

    let session_excerpt = session_excerpt(&prep.messages, session_max_chars);
    let memory_block = if prep.pte_memory_prose.trim().is_empty() {
        String::new()
    } else {
        format!(
            "## Persona memory (Tier 1 / principles excerpt)\n{}\n",
            truncate_chars(prep.pte_memory_prose.trim(), 4000)
        )
    };

    let tools_line = "bash, read_file, write_file, edit_file, grep, glob, search_vault, activate_skill, run_skill_script, web_search, web_fetch";
    let workspace_block = format!(
        "## Workspace\n\
         chat_id={chat_id} persona_id={persona_id}\n\
         tool cwd (persona-scoped): {workspace_display}\n\
         common tools: {tools_line}\n\n"
    );

    PipelineCloudContext {
        known_skill_names,
        skills_catalog,
        session_excerpt,
        memory_block,
        workspace_block,
    }
}

fn session_excerpt(messages: &[Message], max_chars: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    for msg in messages {
        let role = msg.role.as_str();
        let text = message_text(msg);
        if text.trim().is_empty() {
            continue;
        }
        let label = match role {
            "assistant" => "Assistant",
            "user" => "User",
            _ => role,
        };
        parts.push(format!(
            "**{label}:** {}",
            truncate_chars(text.trim(), CLOUD_MESSAGE_MAX_CHARS)
        ));
    }
    if parts.is_empty() {
        return "(no prior messages)".into();
    }
    let mut joined = parts.join("\n\n");
    if joined.chars().count() > max_chars {
        joined = truncate_chars(&joined, max_chars);
        joined.push_str("\n… [conversation excerpt truncated]");
    }
    joined
}

fn message_text(msg: &Message) -> String {
    match &msg.content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}
