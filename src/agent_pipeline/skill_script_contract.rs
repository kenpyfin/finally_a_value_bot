//! Skill script CLI contracts: parse required flags from SKILL.md / scripts and enrich
//! `run_skill_script` args before execution (any skill, any persona).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::skills::SkillManager;
use crate::telegram::AppState;
use crate::tools::run_skill_script::{resolve_script_under_skill_dir, runnable_script_candidates};
use crate::tools::ToolResult;

use super::plan::PlanStep;

#[derive(Debug, Clone)]
pub struct PriorStepSnapshot {
    pub summary: String,
    pub full_output: String,
    pub tool_input_previews: Vec<String>,
    pub tool_result_previews: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCliFlag {
    pub flag: String,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct SkillScriptContract {
    pub skill_name: String,
    pub script: String,
    pub flags: Vec<SkillCliFlag>,
}

/// Prior-step tool output used to backfill missing CLI flag values.
#[derive(Debug, Clone)]
struct ResolveContext<'a> {
    step: &'a PlanStep,
    hints: HashMap<String, String>,
    prior_summaries: Vec<&'a str>,
    prior_tool_previews: Vec<&'a str>,
}

pub fn load_skill_script_contract(
    state: &AppState,
    skill_name: &str,
    script: &str,
) -> Option<SkillScriptContract> {
    let manager = SkillManager::from_skills_dirs(state.config.skill_discovery_dirs());
    let (meta, body) = manager.load_skill(skill_name)?;
    let script_path = resolve_script_under_skill_dir(&meta.dir_path, script).ok()?;
    let mut flags = parse_argparse_flags(&script_path);
    merge_skill_md_flags(&body, &mut flags);
    if flags.is_empty() {
        return None;
    }
    Some(SkillScriptContract {
        skill_name: skill_name.to_string(),
        script: script.to_string(),
        flags,
    })
}

pub fn resolve_default_skill_script(state: &AppState, skill_name: &str) -> Option<String> {
    let manager = SkillManager::from_skills_dirs(state.config.skill_discovery_dirs());
    let (meta, _) = manager.load_skill(skill_name)?;
    let candidates = runnable_script_candidates(&meta.dir_path);
    match candidates.len() {
        0 => None,
        1 => Some(candidates[0].clone()),
        _ => candidates
            .iter()
            .find(|s| s.ends_with("_cli.py"))
            .or_else(|| candidates.iter().find(|s| s.ends_with("_tool.py")))
            .or_else(|| candidates.first())
            .cloned(),
    }
}

pub fn format_contract_args_hint(contract: &SkillScriptContract) -> String {
    let required: Vec<_> = contract
        .flags
        .iter()
        .filter(|f| f.required)
        .map(|f| f.flag.as_str())
        .collect();
    let optional: Vec<_> = contract
        .flags
        .iter()
        .filter(|f| !f.required)
        .map(|f| f.flag.as_str())
        .collect();
    let mut arg_tokens = Vec::new();
    for flag in &required {
        let placeholder = flag.trim_start_matches('-');
        arg_tokens.push(format!("\"{flag}\""));
        arg_tokens.push(format!("\"<{placeholder}>\""));
    }
    let mut hint = format!(
        "run_skill_script(skill_name=\"{}\", script=\"{}\", args=[{}])",
        contract.skill_name,
        contract.script,
        arg_tokens.join(",")
    );
    if !required.is_empty() {
        hint.push_str(&format!(
            " Required CLI flags: {}.",
            required
                .iter()
                .map(|f| format!("{f} <value>"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !optional.is_empty() {
        hint.push_str(&format!(
            " Optional: {}.",
            optional
                .iter()
                .map(|f| format!("{f} <value>"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    hint
}

pub fn contract_required_flags_block(state: &AppState, step: &PlanStep) -> Option<String> {
    let skill = step.skill_name.as_deref()?;
    let script = step.skill_script.as_deref()?;
    let contract = load_skill_script_contract(state, skill, script)?;
    let required: Vec<_> = contract
        .flags
        .iter()
        .filter(|f| f.required)
        .map(|f| format!("{} <value>", f.flag))
        .collect();
    if required.is_empty() {
        None
    } else {
        Some(format!(
            "Contract required CLI flags: {}",
            required.join(" ")
        ))
    }
}

pub fn enrich_run_skill_script_input(
    state: &AppState,
    input: &mut serde_json::Value,
    step: &PlanStep,
    prior_results: &[PriorStepSnapshot],
) {
    let skill_name = input
        .get("skill_name")
        .and_then(|v| v.as_str())
        .or(step.skill_name.as_deref())
        .map(str::to_string);
    let script_name = input
        .get("script")
        .and_then(|v| v.as_str())
        .or(step.skill_script.as_deref())
        .map(str::to_string)
        .or_else(|| {
            skill_name
                .as_deref()
                .and_then(|s| resolve_default_skill_script(state, s))
        });
    let (Some(skill), Some(script)) = (skill_name, script_name) else {
        return;
    };
    input["skill_name"] = serde_json::Value::String(skill.clone());
    input["script"] = serde_json::Value::String(script.clone());

    let Some(contract) = load_skill_script_contract(state, &skill, &script) else {
        return;
    };
    let required: Vec<String> = contract
        .flags
        .iter()
        .filter(|f| f.required)
        .map(|f| f.flag.clone())
        .collect();
    if required.is_empty() {
        return;
    }

    let mut args = args_vec(input);
    let ctx = build_resolve_context(step, prior_results);
    for flag in &required {
        if args_has_flag(&args, flag) {
            continue;
        }
        if let Some(value) = resolve_flag_value(flag, &ctx, &args) {
            args.push(flag.clone());
            args.push(value);
        }
    }
    enrich_optional_output(&contract, &mut args);
    set_args(input, args);
}

pub fn augment_skill_script_result(
    state: &AppState,
    skill_name: Option<&str>,
    script: &str,
    mut result: ToolResult,
) -> ToolResult {
    if !result.is_error {
        return result;
    }
    let missing = parse_missing_required_flags(&result.content);
    if !missing.is_empty() {
        let hint = if let Some(skill) = skill_name {
            load_skill_script_contract(state, skill, script).map(|c| {
                format!(
                    "Missing required CLI flags: {}. {}",
                    missing.join(", "),
                    format_contract_args_hint(&c)
                )
            })
        } else {
            None
        };
        result.content.push_str(&format!(
            "\n\n[skill_runtime] {} Do NOT retry with bash/find/ls as script.",
            hint.unwrap_or_else(|| {
                format!(
                    "Missing required CLI flags: {}. Retry run_skill_script with complete args.",
                    missing.join(", ")
                )
            })
        ));
        return result;
    }
    if (script.ends_with("_cli.py") || script.ends_with("_tool.py"))
        && (result.content.contains("Exit code 1") || result.content.contains("exit code 1"))
    {
        result.content.push_str(
            "\n\n[skill_runtime] Script started but returned a non-zero exit. \
             Retry run_skill_script with adjusted args — do not substitute bash/find/ls as script.",
        );
    }
    result
}

fn enrich_optional_output(contract: &SkillScriptContract, args: &mut Vec<String>) {
    let has_output = contract.flags.iter().any(|f| f.flag == "--output");
    if !has_output || args_has_flag(args, "--output") {
        return;
    }
    let input_flag = ["--input", "--image", "--file", "--path"]
        .iter()
        .find(|f| args_has_flag(args, f))
        .copied();
    let Some(input_flag) = input_flag else {
        return;
    };
    let Some(inp) = arg_value_after_flag(args, input_flag) else {
        return;
    };
    if let Some(out) = derive_sibling_output_path(&inp) {
        args.push("--output".into());
        args.push(out);
    }
}

fn build_resolve_context<'a>(
    step: &'a PlanStep,
    prior_results: &'a [PriorStepSnapshot],
) -> ResolveContext<'a> {
    let mut hints = HashMap::new();
    if let Some(h) = &step.skill_args_hint {
        merge_flag_values_from_text(h, &mut hints);
    }
    merge_flag_values_from_text(&step.inputs, &mut hints);
    merge_flag_values_from_text(&step.goal, &mut hints);

    let mut prior_summaries = Vec::new();
    let mut prior_tool_previews = Vec::new();
    for result in prior_results.iter().rev() {
        let handoff = if result.full_output.trim().is_empty() {
            result.summary.as_str()
        } else {
            result.full_output.as_str()
        };
        prior_summaries.push(handoff);
        for preview in result.tool_input_previews.iter().rev() {
            prior_tool_previews.push(preview.as_str());
        }
        for preview in result.tool_result_previews.iter().rev() {
            prior_tool_previews.push(preview.as_str());
        }
    }

    ResolveContext {
        step,
        hints,
        prior_summaries,
        prior_tool_previews,
    }
}

fn resolve_flag_value(
    flag: &str,
    ctx: &ResolveContext<'_>,
    current_args: &[String],
) -> Option<String> {
    if let Some(v) = ctx.hints.get(flag) {
        if is_concrete_value(v) {
            return Some(v.clone());
        }
    }
    for preview in &ctx.prior_tool_previews {
        if let Some(v) = extract_flag_value_from_preview(flag, preview) {
            if is_concrete_value(&v) {
                return Some(v);
            }
        }
    }
    for summary in &ctx.prior_summaries {
        if let Some(v) = extract_flag_value_from_summary(flag, summary) {
            return Some(v);
        }
    }
    match flag_value_kind(flag) {
        FlagValueKind::Path => resolve_path_flag(flag, ctx, current_args),
        FlagValueKind::Text => resolve_text_flag(flag, ctx),
        FlagValueKind::Other => None,
    }
}

fn resolve_path_flag(
    flag: &str,
    ctx: &ResolveContext<'_>,
    current_args: &[String],
) -> Option<String> {
    let name = flag.trim_start_matches('-');
    if name == "output" {
        for input_flag in ["--input", "--image", "--file", "--path"] {
            if let Some(inp) = arg_value_after_flag(current_args, input_flag) {
                return derive_sibling_output_path(&inp);
            }
        }
        return None;
    }
    for summary in &ctx.prior_summaries {
        if let Some(path) = find_artifact_paths_in_text(summary).into_iter().next() {
            return Some(path);
        }
    }
    for preview in &ctx.prior_tool_previews {
        if let Some(path) = find_artifact_paths_in_text(preview).into_iter().next() {
            return Some(path);
        }
    }
    None
}

fn resolve_text_flag(flag: &str, ctx: &ResolveContext<'_>) -> Option<String> {
    for text in [&ctx.step.goal, &ctx.step.inputs] {
        if let Some(v) = extract_quoted_value_near_flag(flag, text) {
            return Some(v);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlagValueKind {
    Path,
    Text,
    Other,
}

fn flag_value_kind(flag: &str) -> FlagValueKind {
    let name = flag.trim_start_matches('-').to_ascii_lowercase();
    match name.as_str() {
        "input" | "output" | "image" | "file" | "path" | "url" => FlagValueKind::Path,
        "prompt" | "caption" | "query" | "text" | "message" => FlagValueKind::Text,
        _ if name.ends_with("_ref") || name == "ref" || name == "sref" || name == "cref" => {
            FlagValueKind::Path
        }
        _ => FlagValueKind::Other,
    }
}

fn parse_argparse_flags(script_path: &Path) -> Vec<SkillCliFlag> {
    let Ok(body) = std::fs::read_to_string(script_path) else {
        return Vec::new();
    };
    let mut flags = Vec::new();
    for line in body.lines() {
        if !line.contains("add_argument") {
            continue;
        }
        let Some(flag) = extract_long_flag_from_line(line) else {
            continue;
        };
        let required = line.contains("required=True") || line.contains("required = True");
        upsert_flag(&mut flags, flag, required);
    }
    flags
}

fn merge_skill_md_flags(body: &str, flags: &mut Vec<SkillCliFlag>) {
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('-') && !trimmed.starts_with('|') {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        for token in trimmed.split(['`', '|', ',', '(']) {
            let token = token.trim();
            if !token.starts_with("--") {
                continue;
            }
            let flag = token
                .split_whitespace()
                .next()
                .unwrap_or(token)
                .trim_end_matches(':')
                .to_string();
            if flag.len() < 3 {
                continue;
            }
            let required = lower.contains("(required)")
                || lower.contains("**required**")
                || lower.contains("required:");
            upsert_flag(flags, flag, required);
        }
    }
}

fn upsert_flag(flags: &mut Vec<SkillCliFlag>, flag: String, required: bool) {
    if let Some(existing) = flags.iter_mut().find(|f| f.flag == flag) {
        if required {
            existing.required = true;
        }
    } else {
        flags.push(SkillCliFlag { flag, required });
    }
}

fn extract_long_flag_from_line(line: &str) -> Option<String> {
    for part in line.split('"') {
        if part.starts_with("--") && part.len() > 2 {
            let flag = part.split_whitespace().next()?.trim_matches(',');
            return Some(flag.to_string());
        }
    }
    None
}

fn merge_flag_values_from_text(text: &str, out: &mut HashMap<String, String>) {
    if let Some(args_start) = text.find("args=[") {
        let rest = &text[args_start + 6..];
        if let Some(end) = rest.find(']') {
            parse_args_array_pairs(&rest[..end], out);
        }
    }
    for flag in extract_flags_from_text(text) {
        if let Some(v) = extract_flag_value_from_preview(&flag, text) {
            if is_concrete_value(&v) {
                out.insert(flag, v);
            }
        }
    }
}

fn extract_flags_from_text(text: &str) -> Vec<String> {
    let mut flags = Vec::new();
    for token in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
        if token.starts_with("--") && token.len() > 2 && !flags.contains(&token.to_string()) {
            flags.push(token.to_string());
        }
    }
    flags
}

fn parse_args_array_pairs(chunk: &str, out: &mut HashMap<String, String>) {
    let tokens: Vec<&str> = chunk
        .split('"')
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "," && *s != "[")
        .collect();
    let mut i = 0;
    while i + 1 < tokens.len() {
        let key = tokens[i].trim_matches(',');
        let val = tokens[i + 1].trim_matches(',');
        if key.starts_with("--") && is_concrete_value(val) {
            out.insert(key.to_string(), val.to_string());
        }
        i += 2;
    }
}

fn extract_flag_value_from_preview(flag: &str, preview: &str) -> Option<String> {
    let needle = format!("\"{flag}\"");
    if let Some(idx) = preview.find(&needle) {
        let rest = &preview[idx + needle.len()..];
        if let Some(start) = rest.find('"') {
            let rest = &rest[start + 1..];
            if let Some(end) = rest.find('"') {
                let value = rest[..end].trim();
                if is_concrete_value(value) {
                    return Some(value.to_string());
                }
            }
        }
    }
    if flag == "--prompt" {
        if let Some(idx) = preview.find("[*] Prompt:") {
            let rest = preview[idx + 12..].trim();
            let end = rest.find('\n').unwrap_or(rest.len().min(500));
            let value = rest[..end].trim();
            if is_concrete_value(value) {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn extract_flag_value_from_summary(flag: &str, summary: &str) -> Option<String> {
    let key = flag.trim_start_matches('-');
    let patterns = [
        format!("{key}="),
        format!("{}=", key.to_ascii_uppercase()),
        format!("`{flag}`"),
    ];
    for pat in patterns {
        if let Some(idx) = summary.find(&pat) {
            let rest = summary[idx + pat.len()..].trim();
            let end = rest.find([',', '\n', '`']).unwrap_or(rest.len().min(500));
            let value = rest[..end].trim();
            if is_concrete_value(value) {
                return Some(value.to_string());
            }
        }
    }
    if flag_value_kind(flag) == FlagValueKind::Text {
        return extract_quoted_value_near_flag(flag, summary);
    }
    None
}

fn extract_quoted_value_near_flag(flag: &str, text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if !lower.contains(&flag.trim_start_matches('-').to_ascii_lowercase()) {
        return None;
    }
    for segment in text.split('"').skip(1).step_by(2) {
        let trimmed = segment.trim();
        if trimmed.len() >= 15 && is_concrete_value(trimmed) {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn parse_missing_required_flags(stderr: &str) -> Vec<String> {
    let lower = stderr.to_ascii_lowercase();
    let marker = "the following arguments are required:";
    let Some(idx) = lower.find(marker) else {
        return Vec::new();
    };
    let rest = &stderr[idx + marker.len()..];
    let end = rest.find('\n').unwrap_or(rest.len());
    rest[..end]
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| s.starts_with("--"))
        .map(str::to_string)
        .collect()
}

fn find_artifact_paths_in_text(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in text.lines() {
        if let Some(idx) = line.find("ARTIFACT:") {
            let p = line[idx + 9..].trim();
            if looks_like_artifact_path(p) {
                paths.push(p.to_string());
            }
        }
        if let Some(idx) = line.find("saved to ") {
            let rest = line[idx + 9..].trim();
            let end = rest.find([',', '\n']).unwrap_or(rest.len());
            let p = rest[..end].trim();
            if looks_like_artifact_path(p) {
                paths.push(p.to_string());
            }
        }
    }
    for token in text.split_whitespace() {
        let trimmed = trim_path_punctuation(token);
        if looks_like_artifact_path(trimmed) {
            paths.push(trimmed.to_string());
        }
    }
    paths
}

fn looks_like_artifact_path(s: &str) -> bool {
    if s.is_empty() || s.starts_with("http://") || s.starts_with("https://") || s.contains("..") {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".pdf")
        || lower.ends_with(".mp4")
        || s.starts_with("ORIGIN/")
        || s.starts_with("parking/")
        || s.starts_with("shared/")
}

fn trim_path_punctuation(s: &str) -> &str {
    s.trim_matches(|c: char| {
        c == '"' || c == '\'' || c == '(' || c == ')' || c == ',' || c == '`' || c == '*'
    })
}

fn derive_sibling_output_path(input: &str) -> Option<String> {
    let path = PathBuf::from(input);
    let stem = path.file_stem()?.to_string_lossy();
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
    let mut base = stem.to_string();
    for (from, to) in [
        ("-BASE", "-OUTPUT"),
        ("_BASE", "_OUTPUT"),
        ("-base", "-output"),
        ("_base", "_output"),
        ("-Base", "-Output"),
    ] {
        if base.contains(from) {
            base = base.replace(from, to);
            break;
        }
    }
    if base == stem.as_ref() {
        base = format!("{base}_out");
    }
    Some(if let Some(ext) = ext {
        format!("{base}.{ext}")
    } else {
        base
    })
}

fn is_concrete_value(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('<')
        && !value.ends_with('>')
        && value != "..."
        && value != "value"
}

fn args_vec(input: &serde_json::Value) -> Vec<String> {
    input
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn set_args(input: &mut serde_json::Value, args: Vec<String>) {
    input["args"] =
        serde_json::Value::Array(args.into_iter().map(serde_json::Value::String).collect());
}

fn args_has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn arg_value_after_flag(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir() -> PathBuf {
        std::env::temp_dir().join(format!("fab_skill_contract_test_{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn parse_argparse_required_flags() {
        let dir = test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("demo_cli.py"),
            r#"parser.add_argument("--input", required=True)
parser.add_argument("--prompt", required=True, help="x")
parser.add_argument("--output", help="y")
"#,
        )
        .unwrap();
        let flags = parse_argparse_flags(&dir.join("demo_cli.py"));
        assert!(flags.iter().any(|f| f.flag == "--input" && f.required));
        assert!(flags.iter().any(|f| f.flag == "--prompt" && f.required));
        assert!(flags.iter().any(|f| f.flag == "--output" && !f.required));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_missing_required_flags_from_argparse_error() {
        let err = "hotify_cli.py: error: the following arguments are required: --prompt";
        let missing = parse_missing_required_flags(err);
        assert_eq!(missing, vec!["--prompt".to_string()]);
    }

    #[test]
    fn merge_flag_values_from_args_hint() {
        let mut hints = HashMap::new();
        merge_flag_values_from_text(
            r#"args=["--input","base.png","--prompt","dress drop edit"]"#,
            &mut hints,
        );
        assert_eq!(hints.get("--input").map(String::as_str), Some("base.png"));
        assert_eq!(
            hints.get("--prompt").map(String::as_str),
            Some("dress drop edit")
        );
    }

    #[test]
    fn derive_sibling_output_path_replaces_base_suffix() {
        assert_eq!(
            derive_sibling_output_path("PZ-20260628-SUNNY-BASE.png").as_deref(),
            Some("PZ-20260628-SUNNY-OUTPUT.png")
        );
    }

    #[test]
    fn find_artifact_paths_in_text() {
        let paths = find_artifact_paths_in_text(
            "SUCCESS: Result saved to parking/PZ_REF.png\nARTIFACT: /tmp/out.png",
        );
        assert!(paths.iter().any(|p| p.contains("PZ_REF.png")));
        assert!(paths.iter().any(|p| p == "/tmp/out.png"));
    }
}
