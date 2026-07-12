//! Cursor-only prompt shaping: slim system text and resume deltas for sidecar delegation.
//!
//! Does not alter `prepare_agent_run` / `build_system_prompt`; Classic and Deterministic
//! engines are unaffected.

use crate::claude::{ContentBlock, Message, MessageContent};
use crate::cursor_mcp_bridge::MCP_SERVER_NAME;

const TOOL_GROUPS_HEADING: &str = "## Tool groups";
const CONVERSATION_MEMORY_HEADING: &str = "## Conversation Memory";
const AGENT_SKILLS_HEADING: &str = "# Agent Skills";

const MCP_TOOLS_DELEGATION_SECTION: &str = r#"## Tool groups (MCP delegation)
Bot tools are exposed via MCP server `finally-a-value-bot` (loopback). Tool names and parameter schemas come from MCP `tools/list` — do not rely on prose catalogs here.
- **Prefer MCP** for persona-scoped files (`read_file`, `write_file`, `grep`, …), vault (`search_vault`, `read_file`), scheduler, bulletin (`update_bulletin_focus`), channel delivery, and skills (`activate_skill`, `run_skill_script`).
- Cursor built-ins (shell, search) exist; use MCP file tools for paths under the persona cwd and for vault/scheduler/bulletin workflows.
- **Skills:** metadata in the Agent Skills section below is routing only; call `activate_skill` before following procedural skill steps.

"#;

const CURSOR_FILE_DELIVERY_REMINDER: &str = r#"## File links (web delivery)
When sharing files with the user, put markdown links in your **final reply** using **plain absolute local paths** (e.g. `[Spec](/home/.../personas/{chat_id}/{persona_id}/ORIGIN/Projects/spec.md)`).
- Never use `file://` URLs — the web UI cannot open them.
- Never fabricate `/api/uploads/...` URLs — the platform materializes absolute paths at delivery.
- After writing or updating a file, include a markdown link to the **current** absolute path when the user asks for a link.

"#;

const AGENT_SKILLS_INTRO_SLIM: &str = "\n# Agent Skills\n\nMetadata catalog for routing — call **`activate_skill`** to load full `SKILL.md` before procedural steps.\n\n";

const MAX_PROMPT_LEN: usize = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationPromptMode {
    /// Slim system prompt + full conversation messages.
    FullSlim,
    /// Minimal runtime header + delta messages only (resumed Cursor session).
    ResumeDelta,
}

impl DelegationPromptMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FullSlim => "full_slim",
            Self::ResumeDelta => "resume_delta",
        }
    }
}

pub fn select_delegation_prompt_mode(
    resume_agent_id: Option<&str>,
    is_scheduled_task: bool,
    delegation_resume_delta: bool,
) -> DelegationPromptMode {
    if is_scheduled_task {
        return DelegationPromptMode::FullSlim;
    }
    if delegation_resume_delta && resume_agent_id.is_some() {
        DelegationPromptMode::ResumeDelta
    } else {
        DelegationPromptMode::FullSlim
    }
}

/// When MCP is live and slim is enabled, replace the long tool-groups block with MCP delegation prose.
pub fn slim_delegation_system_prompt(full: &str, slim_enabled: bool) -> String {
    if !slim_enabled {
        return full.to_string();
    }
    let mut out = replace_tool_groups_section(full);
    out = shorten_agent_skills_intro(&out);
    if !out.contains("## File links (web delivery)") {
        out.push('\n');
        out.push_str(CURSOR_FILE_DELIVERY_REMINDER);
    }
    out
}

fn replace_tool_groups_section(full: &str) -> String {
    let Some(start) = full.find(TOOL_GROUPS_HEADING) else {
        return full.to_string();
    };
    let after_start = &full[start + TOOL_GROUPS_HEADING.len()..];
    let Some(rel_end) = after_start.find(CONVERSATION_MEMORY_HEADING) else {
        return full.to_string();
    };
    let end = start + TOOL_GROUPS_HEADING.len() + rel_end;
    let mut out = String::with_capacity(full.len());
    out.push_str(&full[..start]);
    out.push_str(MCP_TOOLS_DELEGATION_SECTION);
    out.push_str(&full[end..]);
    out
}

fn shorten_agent_skills_intro(full: &str) -> String {
    let Some(start) = full.find(AGENT_SKILLS_HEADING) else {
        return full.to_string();
    };
    let after_heading = &full[start + AGENT_SKILLS_HEADING.len()..];
    let Some(rel_end) = after_heading.find("<available_skills>") else {
        return full.to_string();
    };
    let end = start + AGENT_SKILLS_HEADING.len() + rel_end;
    let mut out = String::with_capacity(full.len());
    out.push_str(&full[..start]);
    out.push_str(AGENT_SKILLS_INTRO_SLIM);
    out.push_str(&full[end..]);
    out
}

pub struct DelegationRuntimeHeader {
    pub chat_id: i64,
    pub persona_id: i64,
    pub mcp_enabled: bool,
}

fn build_minimal_runtime_header(header: &DelegationRuntimeHeader) -> String {
    let mcp_line = if header.mcp_enabled {
        format!("Tools: MCP server `{MCP_SERVER_NAME}` (authoritative schemas via tools/list).\n")
    } else {
        String::new()
    };
    format!(
        "## Cursor delegation (resume turn)\n\
         chat_id={} persona_id={}\n\
         {mcp_line}\
         Path discipline: never use `workspace/` prefixes; cwd is persona-scoped under \
         `shared/personas/{{chat_id}}/{{persona_id}}/`. Primary task: the `[current_request]` below.\n\n\
         {file_delivery}",
        header.chat_id,
        header.persona_id,
        file_delivery = CURSOR_FILE_DELIVERY_REMINDER,
    )
}

fn is_prior_turn_message(text: &str) -> bool {
    text.contains("context=\"prior_turn\"")
}

fn is_trusted_delta_message(text: &str) -> bool {
    let t = text.trim();
    t.contains("[system_runtime_context")
        || t.starts_with("[persona_context]")
        || t.starts_with("[session_context")
        || t.starts_with("[current_request")
        || t.starts_with("[hook_context]")
}

/// Messages for a resumed Cursor session: runtime/persona/session context, hook steering, current request.
pub fn extract_resume_delta_messages(messages: &[Message]) -> Vec<Message> {
    let mut out = Vec::new();
    for msg in messages {
        if msg.role == "assistant" {
            continue;
        }
        let text = message_text(msg);
        if text.trim().is_empty() || is_prior_turn_message(&text) {
            continue;
        }
        if is_trusted_delta_message(&text) {
            out.push(msg.clone());
        }
    }
    out
}

pub fn build_cursor_delegation_prompt(
    mode: DelegationPromptMode,
    delegation_system: &str,
    messages: &[Message],
    header: &DelegationRuntimeHeader,
    is_scheduled_task: bool,
    has_image_input: bool,
) -> String {
    let mut prompt = match mode {
        DelegationPromptMode::FullSlim => flatten_turn_prompt(delegation_system, messages),
        DelegationPromptMode::ResumeDelta => {
            let delta_messages = extract_resume_delta_messages(messages);
            let runtime_header = build_minimal_runtime_header(header);
            flatten_turn_prompt(&runtime_header, &delta_messages)
        }
    };

    if is_scheduled_task {
        prompt = format!(
            "[scheduled_task]\nAutomated scheduled job — not interactive chat. \
             Complete the task below and return a concise result.\n\n{prompt}"
        );
    }
    if has_image_input {
        prompt.push_str(
            "\n\n(Note: image input was attached but the Cursor engine currently supports text only. \
             Describe any needed visual context in follow-up if required.)\n",
        );
    }
    if prompt.len() > MAX_PROMPT_LEN {
        prompt.truncate(prompt.floor_char_boundary(MAX_PROMPT_LEN));
        prompt.push_str("\n\n(prompt truncated for Cursor engine limit)");
    }
    prompt
}

fn flatten_turn_prompt(system_prompt: &str, messages: &[Message]) -> String {
    let mut out = String::new();
    out.push_str("# System\n\n");
    out.push_str(system_prompt);
    out.push_str("\n\n# Conversation\n\n");
    for msg in messages {
        let text = message_text(msg);
        if text.trim().is_empty() {
            continue;
        }
        out.push_str("## ");
        out.push_str(msg.role.as_str());
        out.push('\n');
        out.push_str(&text);
        out.push_str("\n\n");
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_full_system() -> String {
        format!(
            "# Principles\n\nBe helpful.\n\n{TOOL_GROUPS_HEADING} (names only)\n\
             - **Shell:** bash\n- **Files:** read_file\n\n{CONVERSATION_MEMORY_HEADING}\n\
             - **Primary task**: answer [current_request]\n\n{AGENT_SKILLS_HEADING}\n\n\
             Long intro about metadata and SKILLS_CATALOG_MODE compact full modes.\n\n\
             <available_skills>\n- name: foo\n</available_skills>"
        )
    }

    #[test]
    fn slim_appends_file_delivery_reminder() {
        let full = fixture_full_system();
        let slim = slim_delegation_system_prompt(&full, true);
        assert!(slim.contains("## File links (web delivery)"));
        assert!(slim.contains("Never use `file://` URLs"));
        assert!(slim.contains("Never fabricate `/api/uploads/...` URLs"));
    }

    #[test]
    fn slim_strips_tool_groups_preserves_conversation_memory() {
        let full = fixture_full_system();
        let slim = slim_delegation_system_prompt(&full, true);
        assert!(!slim.contains("- **Shell:** bash"));
        assert!(slim.contains(CONVERSATION_MEMORY_HEADING));
        assert!(slim.contains(MCP_SERVER_NAME));
        assert!(slim.contains("# Principles"));
    }

    #[test]
    fn slim_is_identity_when_disabled() {
        let full = fixture_full_system();
        let slim = slim_delegation_system_prompt(&full, false);
        assert_eq!(slim, full);
    }

    #[test]
    fn slim_shortens_agent_skills_intro() {
        let full = fixture_full_system();
        let slim = slim_delegation_system_prompt(&full, true);
        assert!(!slim.contains("SKILLS_CATALOG_MODE"));
        assert!(slim.contains("<available_skills>"));
    }

    #[test]
    fn delta_includes_trusted_messages_only() {
        let messages = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text(
                    "[system_runtime_context timezone=\"UTC\"]now[/system_runtime_context]".into(),
                ),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Text(
                    "<user_message context=\"prior_turn\">old</user_message>".into(),
                ),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("prior reply".into()),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Text("[persona_context]\nmemo\n[/persona_context]".into()),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Text(
                    "[current_request sender=\"u\"]\ndo it\n[/current_request]".into(),
                ),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Text("[hook_context]\npolicy\n[/hook_context]".into()),
            },
        ];
        let delta = extract_resume_delta_messages(&messages);
        assert_eq!(delta.len(), 4);
        let joined: String = delta
            .iter()
            .map(message_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[persona_context]"));
        assert!(joined.contains("[current_request"));
        assert!(joined.contains("[hook_context]"));
        assert!(!joined.contains("prior_turn"));
        assert!(!joined.contains("prior reply"));
    }

    #[test]
    fn mode_selection_scheduled_always_full_slim() {
        assert_eq!(
            select_delegation_prompt_mode(Some("agent-1"), true, true),
            DelegationPromptMode::FullSlim
        );
    }

    #[test]
    fn mode_selection_resume_uses_delta_when_enabled() {
        assert_eq!(
            select_delegation_prompt_mode(Some("agent-1"), false, true),
            DelegationPromptMode::ResumeDelta
        );
        assert_eq!(
            select_delegation_prompt_mode(Some("agent-1"), false, false),
            DelegationPromptMode::FullSlim
        );
        assert_eq!(
            select_delegation_prompt_mode(None, false, true),
            DelegationPromptMode::FullSlim
        );
    }

    #[test]
    fn resume_delta_prompt_uses_minimal_header() {
        let messages = vec![Message {
            role: "user".into(),
            content: MessageContent::Text(
                "[current_request sender=\"u\"]\nhi\n[/current_request]".into(),
            ),
        }];
        let header = DelegationRuntimeHeader {
            chat_id: 1,
            persona_id: 2,
            mcp_enabled: true,
        };
        let prompt = build_cursor_delegation_prompt(
            DelegationPromptMode::ResumeDelta,
            "unused",
            &messages,
            &header,
            false,
            false,
        );
        assert!(prompt.contains("resume turn"));
        assert!(prompt.contains("chat_id=1"));
        assert!(prompt.contains("## File links (web delivery)"));
        assert!(!prompt.contains("# Principles"));
        assert!(prompt.contains("[current_request]"));
    }
}
