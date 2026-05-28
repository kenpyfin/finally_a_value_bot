//! Shared strict path-discipline text for agent system prompts and skill-creation tools.

/// Full markdown section injected into `build_system_prompt`.
pub fn strict_path_discipline_section(
    tool_cwd_display: &str,
    workspace_data_root_display: &str,
    skills_dir_display: &str,
) -> String {
    format!(
        r##"
## Path discipline (strict)

Follow these rules on **every** turn for `read_file`, `write_file`, `edit_file`, `apply_search_replace`, `symbol_edit`, `glob`, `grep`, **`bash`**, and **`cursor_agent`** / **`build_skill`**.

**Tool cwd:** `{tool_cwd}` (this is `{workspace_data_root_display}/shared/`). Relative tool paths resolve from here—not from the configuration root and not by prefixing `WORKSPACE_DIR` as `workspace/...`.

### Allowed patterns (use these)

| Goal | Path in file/bash tools | Notes |
|------|---------------------------|-------|
| Files in the shared workspace | `ORIGIN/...`, `notes/foo.md`, `./report.pdf` | No `workspace/` prefix |
| Skills (read/write via tools) | `skills/<name>/SKILL.md`, `skills/<name>/script.py` | Resolves to `{skills_dir_display}/` |
| Create or update a skill | **`build_skill`** tool | Writes under `{skills_dir_display}/` |
| Runtime / DB / groups | `runtime/...` | Not `workspace/runtime/...` |
| Skill credentials | `skills/<name>/.env` | Not project-root `.env` via file tools |

### Forbidden patterns (never use)

- `workspace/...`, `workspace/shared/...`, `workspace/skills/...`, `workspace/runtime/...` in tool or bash paths → creates **`shared/workspace/`** (shadow tree; skills there are **not discovered**).
- `shared/skills/...` when cwd is already `shared/` → wrong nesting.
- Bash: `mkdir -p workspace/skills/...`, `cp ... workspace/skills/...`, or any shell path containing `workspace/skills/` while cwd is `shared/`.
- Treating `shared/workspace/` as `WORKSPACE_DIR` or as a canonical skills location.

### Shadow workspace

`{workspace_data_root_display}/shared/workspace/` is a mistaken duplicate. **Do not create or update files there.** Canonical skills live at `{skills_dir_display}/`. If you find shadow copies, migrate to canonical paths and remove shadow duplicates after verifying content.

### Skills checklist

1. Prefer **`build_skill(name=..., ...)`** over manual `write_file` under `skills/`.
2. If using `write_file`, path must be `skills/<name>/SKILL.md` (tool-relative), never `workspace/skills/...`.
3. Skill shell examples in SKILL.md bodies should use `skills/<name>/script.py` or absolute `{skills_dir_display}/<name>/...`, not `workspace/skills/...`.
4. After creating a skill, it must exist at `{skills_dir_display}/<name>/SKILL.md` to be discoverable by `/skills` and `activate_skill`.
"##,
        tool_cwd = tool_cwd_display,
        workspace_data_root_display = workspace_data_root_display,
        skills_dir_display = skills_dir_display,
    )
}

/// Short footer for `build_skill` / cursor-agent skill-creation prompts.
pub fn build_skill_path_discipline_footer(skills_dir_display: &str, skill_name: &str) -> String {
    format!(
        r#"
PATH DISCIPLINE (mandatory):
- Write ONLY under: {skills_dir_display}/{skill_name}/
- Create or overwrite: {skills_dir_display}/{skill_name}/SKILL.md
- Do NOT write under shared/workspace/, workspace/skills/, or any path that prefixes workspace/ when cwd is shared/.
- Use the absolute paths above; do not create a nested shared/workspace/ tree.
"#,
        skills_dir_display = skills_dir_display,
        skill_name = skill_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_section_forbids_workspace_skills_prefix() {
        let s = strict_path_discipline_section("/data/shared", "/data", "/data/skills");
        assert!(s.contains("## Path discipline (strict)"));
        assert!(s.contains("workspace/skills/"));
        assert!(s.contains("shared/workspace/"));
        assert!(s.contains("/data/skills"));
    }

    #[test]
    fn build_skill_footer_uses_absolute_skill_dir() {
        let s = build_skill_path_discipline_footer("/data/skills", "my-skill");
        assert!(s.contains("/data/skills/my-skill/SKILL.md"));
        assert!(s.contains("Do NOT write under shared/workspace/"));
    }
}
