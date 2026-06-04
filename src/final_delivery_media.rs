//! Shared pre-delivery artifact resolution and bare-filename normalization for all channels.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactResolveKind {
    /// PNG/JPEG/GIF/WebP/BMP only (Telegram auto-photos, bare-line normalization).
    ImageOnly,
    /// Any existing file (web link materialization).
    AnyFile,
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

fn trim_artifact_ref(raw: &str) -> &str {
    raw.trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>')
}

pub fn is_deliverable_image_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some(ext) if IMAGE_EXTENSIONS.contains(&ext)
    )
}

fn path_matches_kind(path: &Path, kind: ArtifactResolveKind) -> bool {
    if !path.is_file() {
        return false;
    }
    match kind {
        ArtifactResolveKind::AnyFile => true,
        ArtifactResolveKind::ImageOnly => is_deliverable_image_extension(path),
    }
}

fn path_under_workspace(
    candidate: &Path,
    workspace_root: &Path,
    kind: ArtifactResolveKind,
) -> Option<PathBuf> {
    let cand = candidate.canonicalize().ok()?;
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    if !cand.starts_with(&root) || !path_matches_kind(&cand, kind) {
        return None;
    }
    Some(cand)
}

fn collect_artifact_candidates(
    workspace_root: &Path,
    chat_id: i64,
    persona_id: i64,
    trimmed: &str,
) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let as_path = PathBuf::from(trimmed);
    if as_path.is_absolute() {
        candidates.push(as_path);
    } else {
        candidates.push(workspace_root.join(trimmed));
        candidates.push(workspace_root.join("shared").join(trimmed));

        if let Some(stripped) = trimmed.strip_prefix("./") {
            candidates.push(workspace_root.join(stripped));
            candidates.push(workspace_root.join("shared").join(stripped));
        }
        if let Some(stripped) = trimmed.strip_prefix("shared/") {
            candidates.push(workspace_root.join("shared").join(stripped));
        }

        let persona_dir = workspace_root
            .join("shared")
            .join("personas")
            .join(chat_id.to_string())
            .join(persona_id.to_string());
        candidates.push(persona_dir.join(trimmed));
        candidates.push(
            workspace_root
                .join("runtime")
                .join("groups")
                .join(chat_id.to_string())
                .join(persona_id.to_string())
                .join(trimmed),
        );

        if let Some(repo_root) = workspace_root.parent() {
            candidates.push(repo_root.join(trimmed));
            if let Some(stripped) = trimmed.strip_prefix("workspace/") {
                candidates.push(repo_root.join(stripped));
            }
        }
    }
    candidates
}

/// Depth-limited search for a unique basename under the persona directory.
fn unique_persona_basename_match(
    workspace_root: &Path,
    chat_id: i64,
    persona_id: i64,
    basename: &str,
    kind: ArtifactResolveKind,
) -> Option<PathBuf> {
    let persona_root = workspace_root
        .join("shared")
        .join("personas")
        .join(chat_id.to_string())
        .join(persona_id.to_string());
    if !persona_root.is_dir() {
        return None;
    }
    let mut matches: Vec<PathBuf> = Vec::new();
    collect_basename_matches(&persona_root, basename, kind, 0, 4, &mut matches);
    if matches.len() == 1 {
        path_under_workspace(&matches[0], workspace_root, kind)
    } else {
        None
    }
}

fn collect_basename_matches(
    dir: &Path,
    basename: &str,
    kind: ArtifactResolveKind,
    depth: u32,
    max_depth: u32,
    out: &mut Vec<PathBuf>,
) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == basename)
                && path_matches_kind(&path, kind)
            {
                out.push(path);
            }
        } else if path.is_dir() {
            collect_basename_matches(&path, basename, kind, depth + 1, max_depth, out);
        }
    }
}

/// Resolve a local workspace artifact path (persona-scoped when `chat_id` / `persona_id` are set).
pub fn resolve_workspace_artifact_path(
    workspace_root: &Path,
    chat_id: Option<i64>,
    persona_id: Option<i64>,
    raw: &str,
    kind: ArtifactResolveKind,
) -> Option<PathBuf> {
    let t = trim_artifact_ref(raw);
    if t.is_empty()
        || t.starts_with("http://")
        || t.starts_with("https://")
        || t.starts_with("data:")
        || t.starts_with("/api/uploads/")
    {
        return None;
    }

    if t.starts_with('/') {
        return path_under_workspace(&PathBuf::from(t), workspace_root, kind);
    }

    if let (Some(cid), Some(pid)) = (chat_id, persona_id) {
        for candidate in collect_artifact_candidates(workspace_root, cid, pid, t) {
            if path_matches_kind(&candidate, kind) {
                if let Some(p) = path_under_workspace(&candidate, workspace_root, kind) {
                    return Some(p);
                }
            }
        }
        if !t.contains('/') && !t.contains('\\') {
            if let Some(p) = unique_persona_basename_match(workspace_root, cid, pid, t, kind) {
                return Some(p);
            }
        }
    }

    // Legacy resolution without persona scope.
    let shared = workspace_root.join("shared").join(t);
    let p = if shared.exists() {
        shared
    } else {
        workspace_root.join(t)
    };
    path_under_workspace(&p, workspace_root, kind)
}

fn bare_image_line_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?m)^[ \t]*(?P<name>[^\s/\\]+\.(?:png|jpg|jpeg|gif|webp|bmp))(?:[ \t]+\([^)]+\))?[ \t]*$",
        )
        .unwrap()
    })
}

fn line_should_skip_bare_normalize(line: &str) -> bool {
    let t = line.trim();
    t.is_empty()
        || t.contains("![")
        || t.contains("/api/uploads/")
        || t.starts_with("http://")
        || t.starts_with("https://")
}

/// Rewrite bare image filename lines into markdown images with canonical absolute paths.
pub fn normalize_assistant_artifact_references(
    text: &str,
    workspace_root: &Path,
    chat_id: i64,
    persona_id: i64,
) -> String {
    let mut out = text.to_string();
    for line in text.lines() {
        if line_should_skip_bare_normalize(line) {
            continue;
        }
        let Some(caps) = bare_image_line_regex().captures(line.trim()) else {
            continue;
        };
        let Some(name_m) = caps.name("name") else {
            continue;
        };
        let basename = name_m.as_str();
        let Some(path) = resolve_workspace_artifact_path(
            workspace_root,
            Some(chat_id),
            Some(persona_id),
            basename,
            ArtifactResolveKind::ImageOnly,
        ) else {
            continue;
        };
        let md = format!("![{basename}]({})", path.display());
        if line == line.trim() {
            out = out.replace(line, &md);
        } else {
            out = out.replace(line.trim(), &md);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("fdm_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("shared").join("personas").join("1").join("2")).unwrap();
        root
    }

    #[test]
    fn resolve_persona_scoped_basename() {
        let root = temp_workspace();
        let img = root.join("shared/personas/1/2/PZ-foo.png");
        fs::write(&img, [137u8, 80, 78, 71, 13, 10, 26, 10]).unwrap();
        let root = root.canonicalize().unwrap();

        let resolved = resolve_workspace_artifact_path(
            &root,
            Some(1),
            Some(2),
            "PZ-foo.png",
            ArtifactResolveKind::ImageOnly,
        );
        assert!(resolved.is_some());
        assert!(resolved.unwrap().ends_with(Path::new("PZ-foo.png")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn normalize_bare_filename_line() {
        let root = temp_workspace();
        let img = root.join("shared/personas/1/2/PZ-foo.png");
        fs::write(&img, [137u8, 80, 78, 71, 13, 10, 26, 10]).unwrap();
        let root = root.canonicalize().unwrap();

        let input = "Hello\n\nPZ-foo.png (Refined Identity)\n\nBye";
        let out = normalize_assistant_artifact_references(&input, &root, 1, 2);
        assert!(out.contains("!("));
        assert!(out.contains("PZ-foo.png"));
        assert!(!out.contains("PZ-foo.png (Refined Identity)"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn normalize_skips_existing_markdown() {
        let root = temp_workspace();
        let input = "![alt](https://example.com/a.png)";
        let out = normalize_assistant_artifact_references(&input, &root, 1, 2);
        assert_eq!(out, input);
        let _ = fs::remove_dir_all(root);
    }
}
