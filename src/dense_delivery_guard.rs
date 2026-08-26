//! Persona-gated dense delivery: spill over-limit replies to PDF + public URL.
//!
//! v1 is a **length gate only**. Research/tables under the cap stay LLM/SOP until a later
//! always-spill policy. Runs after PDQE so quality eval sees the full assistant text.

use std::ffi::OsStr;
use std::path::Path;

use async_trait::async_trait;
use chrono::Utc;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::{Database, Persona};

pub const DEFAULT_MESSAGING_MAX_CHARS: usize = 2000;
pub const DEFAULT_WEB_MAX_CHARS: usize = 1000;
pub const DEFAULT_SUMMARY_CHARS: usize = 800;

const CATBOX_UPLOAD_URL: &str = "https://catbox.moe/user/api.php";
const PDF_RENDER_TIMEOUT_SECS: u64 = 60;
const UPLOAD_TIMEOUT_SECS: u64 = 30;

#[async_trait]
pub trait DeliveryUploader: Send + Sync {
    async fn upload_file(&self, path: &Path) -> Result<String, String>;
}

#[async_trait]
pub trait PdfRenderer: Send + Sync {
    async fn render_pdf(
        &self,
        persona_cwd: &Path,
        md_path: &Path,
        pdf_path: &Path,
    ) -> Result<(), String>;
}

pub struct CatboxUploader {
    client: reqwest::Client,
}

impl CatboxUploader {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(UPLOAD_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }
}

impl Default for CatboxUploader {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DeliveryUploader for CatboxUploader {
    async fn upload_file(&self, path: &Path) -> Result<String, String> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("delivery.bin")
            .to_string();
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| format!("failed to read spill file: {e}"))?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name)
            .mime_str("application/octet-stream")
            .map_err(|e| format!("invalid multipart mime: {e}"))?;
        let form = reqwest::multipart::Form::new()
            .text("reqtype", "fileupload")
            .part("fileToUpload", part);
        let resp = self
            .client
            .post(CATBOX_UPLOAD_URL)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("catbox request failed: {e}"))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("catbox response body: {e}"))?;
        let url = body.trim().to_string();
        if !status.is_success() {
            return Err(format!("catbox HTTP {status}: {url}"));
        }
        validate_public_https_url(&url).map_err(|e| e.to_string())?;
        Ok(url)
    }
}

pub struct NoopUploader;

#[async_trait]
impl DeliveryUploader for NoopUploader {
    async fn upload_file(&self, _path: &Path) -> Result<String, String> {
        Err("DELIVERY_UPLOAD_PROVIDER=none".into())
    }
}

pub struct RealPdfRenderer;

#[async_trait]
impl PdfRenderer for RealPdfRenderer {
    async fn render_pdf(
        &self,
        persona_cwd: &Path,
        md_path: &Path,
        pdf_path: &Path,
    ) -> Result<(), String> {
        let script = persona_cwd.join("md2pdf_cjk.py");
        if script.is_file() {
            return run_timed_command(
                "python3",
                &[
                    script.as_os_str(),
                    md_path.as_os_str(),
                    pdf_path.as_os_str(),
                ],
                Some(persona_cwd),
            )
            .await;
        }
        run_timed_command(
            "pandoc",
            &[md_path.as_os_str(), OsStr::new("-o"), pdf_path.as_os_str()],
            Some(persona_cwd),
        )
        .await
    }
}

async fn run_timed_command(
    program: &str,
    args: &[&OsStr],
    cwd: Option<&Path>,
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn {program}: {e}"))?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(PDF_RENDER_TIMEOUT_SECS),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| format!("{program} timed out after {PDF_RENDER_TIMEOUT_SECS}s"))?
    .map_err(|e| format!("{program} wait failed: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{program} exited {}: {}",
            output.status,
            stderr.trim()
        ))
    }
}

pub fn is_messaging_channel(caller_channel: &str) -> bool {
    matches!(
        caller_channel.trim().to_ascii_lowercase().as_str(),
        "telegram" | "discord" | "whatsapp" | "wecom"
    )
}

pub fn effective_max_chars(persona: &Persona, caller_channel: &str) -> usize {
    if is_messaging_channel(caller_channel) {
        positive_or_default(
            persona.dense_delivery_messaging_max_chars,
            DEFAULT_MESSAGING_MAX_CHARS,
        )
    } else {
        positive_or_default(persona.dense_delivery_web_max_chars, DEFAULT_WEB_MAX_CHARS)
    }
}

pub fn effective_summary_chars(persona: &Persona) -> usize {
    positive_or_default(persona.dense_delivery_summary_chars, DEFAULT_SUMMARY_CHARS)
}

fn positive_or_default(value: Option<i64>, default: usize) -> usize {
    value
        .and_then(|n| (n > 0).then_some(n as usize))
        .unwrap_or(default)
}

pub fn upload_provider_from_env() -> &'static str {
    match std::env::var("DELIVERY_UPLOAD_PROVIDER") {
        Ok(v) if v.trim().eq_ignore_ascii_case("none") => "none",
        _ => "catbox",
    }
}

fn uploader_from_env() -> Box<dyn DeliveryUploader> {
    if upload_provider_from_env() == "none" {
        Box::new(NoopUploader)
    } else {
        Box::new(CatboxUploader::new())
    }
}

pub fn validate_public_https_url(url: &str) -> Result<&str, &'static str> {
    let trimmed = url.trim();
    if trimmed.starts_with("https://")
        && !trimmed.contains(char::is_whitespace)
        && trimmed.len() > "https://".len()
    {
        Ok(trimmed)
    } else {
        Err("upload response was not a public https URL")
    }
}

/// Apply dense delivery when the persona toggle is on and text exceeds the channel cap.
/// Returns `Some(summary)` when the delivered text should be replaced.
pub async fn maybe_apply_dense_delivery(
    config: &Config,
    db: &Database,
    chat_id: i64,
    persona_id: i64,
    caller_channel: &str,
    text: &str,
) -> Option<String> {
    let persona = db.get_persona(persona_id).ok().flatten()?;
    if persona.chat_id != chat_id {
        return None;
    }
    maybe_apply_dense_delivery_with(
        config,
        &persona,
        caller_channel,
        text,
        uploader_from_env().as_ref(),
        &RealPdfRenderer,
    )
    .await
}

pub async fn maybe_apply_dense_delivery_with(
    config: &Config,
    persona: &Persona,
    caller_channel: &str,
    text: &str,
    uploader: &dyn DeliveryUploader,
    pdf: &dyn PdfRenderer,
) -> Option<String> {
    if !persona.dense_delivery_enabled {
        return None;
    }
    let cleaned = text.trim();
    let max_chars = effective_max_chars(persona, caller_channel);
    let char_count = cleaned.chars().count();
    if char_count <= max_chars {
        return None;
    }

    let workspace_root = config.workspace_root_absolute();
    let persona_cwd =
        crate::tools::persona_shared_dir(&workspace_root, persona.chat_id, persona.id);
    let delivery_dir = persona_cwd.join("delivery");
    if let Err(e) = std::fs::create_dir_all(&delivery_dir) {
        warn!(
            chat_id = persona.chat_id,
            persona_id = persona.id,
            error = %e,
            "dense_delivery: failed to create delivery dir; summarizing without file"
        );
        return Some(build_summary(
            &cleaned,
            None,
            effective_summary_chars(persona),
            char_count,
            is_messaging_channel(caller_channel),
            None,
        ));
    }

    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let stem = format!("response-{stamp}");
    let md_path = delivery_dir.join(format!("{stem}.md"));
    if let Err(e) = std::fs::write(&md_path, cleaned.as_bytes()) {
        warn!(
            chat_id = persona.chat_id,
            persona_id = persona.id,
            error = %e,
            "dense_delivery: failed to write markdown spill"
        );
        return Some(build_summary(
            &cleaned,
            None,
            effective_summary_chars(persona),
            char_count,
            is_messaging_channel(caller_channel),
            None,
        ));
    }

    let pdf_path = delivery_dir.join(format!("{stem}.pdf"));
    let upload_path = match pdf.render_pdf(&persona_cwd, &md_path, &pdf_path).await {
        Ok(()) if pdf_path.is_file() => pdf_path,
        Ok(()) => {
            warn!(
                chat_id = persona.chat_id,
                persona_id = persona.id,
                "dense_delivery: PDF renderer succeeded but file missing; uploading markdown"
            );
            md_path.clone()
        }
        Err(e) => {
            warn!(
                chat_id = persona.chat_id,
                persona_id = persona.id,
                error = %e,
                "dense_delivery: PDF render failed; uploading markdown"
            );
            md_path.clone()
        }
    };

    let public_url = match uploader.upload_file(&upload_path).await {
        Ok(url) => match validate_public_https_url(&url) {
            Ok(u) => Some(u.to_string()),
            Err(e) => {
                warn!(
                    chat_id = persona.chat_id,
                    persona_id = persona.id,
                    error = e,
                    "dense_delivery: upload URL rejected"
                );
                None
            }
        },
        Err(e) => {
            warn!(
                chat_id = persona.chat_id,
                persona_id = persona.id,
                error = %e,
                "dense_delivery: upload failed"
            );
            None
        }
    };

    let local_link = if !is_messaging_channel(caller_channel) {
        Some(upload_path)
    } else {
        None
    };

    let summary = build_summary(
        &cleaned,
        public_url.as_deref(),
        effective_summary_chars(persona),
        char_count,
        is_messaging_channel(caller_channel),
        local_link.as_deref(),
    );
    info!(
        chat_id = persona.chat_id,
        persona_id = persona.id,
        channel = caller_channel,
        chars = char_count,
        max_chars,
        has_public_url = public_url.is_some(),
        "dense_delivery_spill"
    );
    Some(summary)
}

fn build_summary(
    full_text: &str,
    public_url: Option<&str>,
    summary_chars: usize,
    original_chars: usize,
    messaging: bool,
    local_path: Option<&Path>,
) -> String {
    let topic = extract_topic(full_text);
    let bullets = extract_heading_bullets(full_text, 5);
    let mut out = String::new();
    out.push_str(&topic);
    out.push_str(" — summary\n\n");
    if bullets.is_empty() {
        let excerpt = first_plain_excerpt(full_text, 400);
        if !excerpt.is_empty() {
            out.push_str("• ");
            out.push_str(&excerpt);
            out.push('\n');
        }
    } else {
        for b in bullets {
            out.push_str("• ");
            out.push_str(&b);
            out.push('\n');
        }
    }
    out.push('\n');
    out.push_str(&format!(
        "Full report: {original_chars} characters (spilled to PDF).\n"
    ));
    if let Some(url) = public_url {
        out.push_str("PDF: ");
        out.push_str(url);
        out.push('\n');
    } else if messaging {
        out.push_str("Public upload failed; full report was not pasted inline.\n");
    } else if let Some(path) = local_path {
        let title = topic.trim();
        out.push_str(&format!("[{title}]({})\n", path.display()));
    } else {
        out.push_str("Public upload failed; full report stays in persona delivery/.\n");
    }

    let mut text = if messaging {
        strip_local_path_leaks(&out)
    } else {
        out
    };
    if text.chars().count() > summary_chars {
        text = truncate_chars(&text, summary_chars.saturating_sub(1));
        text.push('…');
        if messaging {
            text = strip_local_path_leaks(&text);
        }
    }
    text
}

fn extract_topic(text: &str) -> String {
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let heading = t.trim_start_matches('#').trim();
        if !heading.is_empty() {
            return truncate_chars(heading, 80);
        }
    }
    "Report".into()
}

fn extract_heading_bullets(text: &str, max: usize) -> Vec<String> {
    let mut bullets = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("## ") {
            let item = rest.trim();
            if !item.is_empty() {
                bullets.push(truncate_chars(item, 120));
            }
        }
        if bullets.len() >= max {
            break;
        }
    }
    bullets
}

fn first_plain_excerpt(text: &str, max_chars: usize) -> String {
    let mut buf = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with('|') {
            continue;
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(t);
        if buf.chars().count() >= max_chars {
            break;
        }
    }
    truncate_chars(&buf, max_chars)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        input.to_string()
    } else {
        input.chars().take(max_chars).collect()
    }
}

fn strip_local_path_leaks(text: &str) -> String {
    let mut out = text.to_string();
    for needle in ["/home/", "/api/uploads/", "file://"] {
        while let Some(idx) = out.find(needle) {
            let rest = &out[idx..];
            let end = rest
                .find(|c: char| c.is_whitespace() || c == ')' || c == ']' || c == '>')
                .unwrap_or(rest.len());
            out.replace_range(idx..idx + end, "[local path omitted]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sample_persona(enabled: bool) -> Persona {
        Persona {
            id: 28,
            chat_id: 997894126,
            name: "selling_oversea".into(),
            model_override: None,
            recent_history_min_user: None,
            recent_history_min_assistant: None,
            operator_memo: None,
            dense_delivery_enabled: enabled,
            dense_delivery_messaging_max_chars: None,
            dense_delivery_web_max_chars: None,
            dense_delivery_summary_chars: None,
            agent_engine_override: None,
        }
    }

    struct StubUploader {
        url: String,
        calls: AtomicUsize,
    }

    impl StubUploader {
        fn new(url: &str) -> Self {
            Self {
                url: url.to_string(),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl DeliveryUploader for StubUploader {
        async fn upload_file(&self, _path: &Path) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.url.clone())
        }
    }

    struct StubPdf;

    #[async_trait]
    impl PdfRenderer for StubPdf {
        async fn render_pdf(
            &self,
            _persona_cwd: &Path,
            _md_path: &Path,
            pdf_path: &Path,
        ) -> Result<(), String> {
            tokio::fs::write(pdf_path, b"%PDF-1.4\n")
                .await
                .map_err(|e| e.to_string())
        }
    }

    fn test_cfg(root: &Path) -> Config {
        let mut cfg = crate::config::test_config();
        cfg.workspace_dir = root.to_string_lossy().into_owned();
        cfg
    }

    fn long_report() -> String {
        let mut s = String::from("# Trade memo\n\n## Finding one\n\n");
        s.push_str(&"lorem ipsum dolor sit amet. ".repeat(120));
        s.push_str("\n\n## Finding two\n\nMore analysis.\n");
        s.push_str("\n\n## Finding three\n\nTables omitted.\n");
        assert!(s.chars().count() > DEFAULT_MESSAGING_MAX_CHARS);
        s
    }

    #[tokio::test]
    async fn toggle_off_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path());
        let persona = sample_persona(false);
        let up = StubUploader::new("https://files.catbox.moe/example.pdf");
        let out =
            maybe_apply_dense_delivery_with(&cfg, &persona, "wecom", &long_report(), &up, &StubPdf)
                .await;
        assert!(out.is_none());
        assert_eq!(up.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn under_limit_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path());
        let persona = sample_persona(true);
        let up = StubUploader::new("https://files.catbox.moe/example.pdf");
        let out =
            maybe_apply_dense_delivery_with(&cfg, &persona, "wecom", "short reply", &up, &StubPdf)
                .await;
        assert!(out.is_none());
        assert_eq!(up.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn over_limit_summary_contains_https() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path());
        let persona = sample_persona(true);
        let up = StubUploader::new("https://files.catbox.moe/example.pdf");
        let out =
            maybe_apply_dense_delivery_with(&cfg, &persona, "wecom", &long_report(), &up, &StubPdf)
                .await
                .expect("spill");
        assert!(out.contains("https://"));
        assert!(out.contains("files.catbox.moe"));
        assert!(out.contains("Trade memo"));
        assert_eq!(up.calls.load(Ordering::SeqCst), 1);
        let delivery = crate::tools::persona_shared_dir(tmp.path(), persona.chat_id, persona.id)
            .join("delivery");
        let entries: Vec<_> = std::fs::read_dir(&delivery).unwrap().collect();
        assert!(!entries.is_empty());
    }

    #[tokio::test]
    async fn messaging_output_never_contains_local_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path());
        let persona = sample_persona(true);
        let up = StubUploader::new("https://files.catbox.moe/example.pdf");
        let mut body = long_report();
        body.push_str("\nSee /home/operator/secret.md and /api/uploads/x.pdf\n");
        let out = maybe_apply_dense_delivery_with(&cfg, &persona, "wecom", &body, &up, &StubPdf)
            .await
            .expect("spill");
        assert!(!out.contains("/home/"));
        assert!(!out.contains("/api/uploads/"));
        assert!(out.contains("https://"));
    }

    #[tokio::test]
    async fn persona_limit_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path());
        let mut persona = sample_persona(true);
        persona.dense_delivery_messaging_max_chars = Some(10);
        let up = StubUploader::new("https://files.catbox.moe/tiny.pdf");
        let out = maybe_apply_dense_delivery_with(
            &cfg,
            &persona,
            "telegram",
            "this is more than ten",
            &up,
            &StubPdf,
        )
        .await;
        assert!(out.is_some());
        assert_eq!(up.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn invalid_upload_url_rejected() {
        assert!(validate_public_https_url("http://files.catbox.moe/x.pdf").is_err());
        assert!(validate_public_https_url("/api/uploads/x.pdf").is_err());
        assert!(validate_public_https_url("https://files.catbox.moe/x.pdf").is_ok());
    }

    #[test]
    fn messaging_channel_classification() {
        assert!(is_messaging_channel("wecom"));
        assert!(is_messaging_channel("Telegram"));
        assert!(!is_messaging_channel("web"));
        assert!(!is_messaging_channel("scheduler"));
    }
}
