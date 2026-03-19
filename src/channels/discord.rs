use std::path::Path;
use std::sync::Arc;

use serenity::async_trait;
use serenity::model::channel::Message as DiscordMessage;
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use base64::Engine;
use tracing::{error, info};

use crate::claude::Message as ClaudeMessage;
use crate::db::call_blocking;
use crate::db::StoredMessage;
use crate::slash_commands::{parse as parse_slash_command, SlashCommand};
use crate::telegram::{archive_conversation, AgentRequestContext, AppState};

struct Handler {
    app_state: Arc<AppState>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: DiscordMessage) {
        // Ignore messages from bots (including ourselves)
        if msg.author.bot {
            return;
        }

        let mut text = msg.content.clone();
        let channel_id = msg.channel_id.get() as i64;
        let channel_handle = channel_id.to_string();
        let sender_name = msg.author.name.clone();
        let mut image_data: Option<(String, String)> = None;
        let mut attachment_notes: Vec<String> = Vec::new();

        if !msg.attachments.is_empty() {
            let max_bytes = self
                .app_state
                .config
                .max_document_size_mb
                .saturating_mul(1024)
                .saturating_mul(1024);
            let upload_dir = Path::new(self.app_state.config.working_dir())
                .join("uploads")
                .join("discord")
                .join(channel_id.to_string());
            if let Err(e) = std::fs::create_dir_all(&upload_dir) {
                error!(
                    "Failed to create Discord upload dir {}: {e}",
                    upload_dir.display()
                );
            } else {
                for (idx, attachment) in msg.attachments.iter().enumerate() {
                    let size = attachment.size as u64;
                    let mime = attachment
                        .content_type
                        .clone()
                        .unwrap_or_else(|| "application/octet-stream".to_string());
                    if size > max_bytes {
                        attachment_notes.push(format!(
                            "[document] filename={} bytes={} mime={} skipped=too_large",
                            attachment.filename, size, mime
                        ));
                        continue;
                    }

                    match reqwest::get(attachment.url.as_str()).await {
                        Ok(resp) => match resp.bytes().await {
                            Ok(bytes) => {
                                let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
                                let safe_name = sanitize_upload_filename(&attachment.filename);
                                let path =
                                    upload_dir.join(format!("{}-{}-{}", ts, idx + 1, safe_name));
                                if let Err(e) = tokio::fs::write(&path, &bytes).await {
                                    error!(
                                        "Failed to save Discord attachment {}: {e}",
                                        path.display()
                                    );
                                    attachment_notes.push(format!(
                                        "[document] filename={} bytes={} mime={} save_error={}",
                                        attachment.filename,
                                        bytes.len(),
                                        mime,
                                        e
                                    ));
                                    continue;
                                }

                                if image_data.is_none() && mime.starts_with("image/") {
                                    let b64 = base64::engine::general_purpose::STANDARD
                                        .encode(bytes.as_ref());
                                    image_data = Some((b64, mime.clone()));
                                }

                                attachment_notes.push(format!(
                                    "[document] filename={} bytes={} mime={} saved_path={}",
                                    attachment.filename,
                                    bytes.len(),
                                    mime,
                                    path.display()
                                ));
                            }
                            Err(e) => {
                                attachment_notes.push(format!(
                                    "[document] filename={} bytes={} mime={} download_error={}",
                                    attachment.filename, size, mime, e
                                ));
                            }
                        },
                        Err(e) => {
                            attachment_notes.push(format!(
                                "[document] filename={} bytes={} mime={} download_error={}",
                                attachment.filename, size, mime, e
                            ));
                        }
                    }
                }
            }
        }

        if !attachment_notes.is_empty() {
            let notes = attachment_notes.join("\n");
            if text.trim().is_empty() {
                text = notes;
            } else {
                text = format!("{}\n\n{}", text.trim(), notes);
            }
        }

        // Resolve to unified contact (canonical_chat_id).
        // When UNIVERSAL_CHAT_ID is configured, bind this Discord handle to that canonical contact.
        let universal_chat_id = self.app_state.config.universal_chat_id;
        let canonical_chat_id = match call_blocking(self.app_state.db.clone(), move |db| {
            if let Some(cid) = universal_chat_id {
                db.upsert_chat(cid, None, "discord")?;
                db.link_channel(cid, "discord", &channel_handle)?;
                Ok(cid)
            } else {
                db.resolve_canonical_chat_id("discord", &channel_handle, None)
            }
        })
        .await
        {
            Ok(cid) => cid,
            Err(e) => {
                error!("Discord resolve_canonical_chat_id: {e}");
                return;
            }
        };

        // Check allowed channels (empty = all)
        if !self.app_state.config.discord_allowed_channels.is_empty()
            && !self
                .app_state
                .config
                .discord_allowed_channels
                .contains(&(channel_id as u64))
        {
            return;
        }

        // Single entry point: parse slash command first. If command, run backend handler and return — never send to LLM.
        if let Some(cmd) = parse_slash_command(&text) {
            match cmd {
                SlashCommand::Reset => {
                    let pid = call_blocking(self.app_state.db.clone(), move |db| db.get_current_persona_id(canonical_chat_id)).await.unwrap_or(0);
                    if pid > 0 {
                        let _ = call_blocking(self.app_state.db.clone(), move |db| db.delete_session(canonical_chat_id, pid)).await;
                    }
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            "Conversation cleared. Principles and per-persona memory are unchanged.",
                        )
                        .await;
                }
                SlashCommand::Skills => {
                    let formatted = self.app_state.skills.list_skills_formatted();
                    let _ = msg.channel_id.say(&ctx.http, &formatted).await;
                }
                SlashCommand::Persona => {
                    let resp = crate::persona::handle_persona_command(self.app_state.db.clone(), canonical_chat_id, text.trim(), Some(&self.app_state.config)).await;
                    let _ = msg.channel_id.say(&ctx.http, resp).await;
                }
                SlashCommand::Schedule => {
                    let tasks = call_blocking(self.app_state.db.clone(), |db| db.get_all_scheduled_tasks_for_display()).await;
                    let text = match &tasks {
                        Ok(t) => crate::tools::schedule::format_tasks_list_all(t),
                        Err(e) => format!("Error listing tasks: {e}"),
                    };
                    let _ = msg.channel_id.say(&ctx.http, &text).await;
                }
                SlashCommand::Archive => {
                    let pid = call_blocking(self.app_state.db.clone(), move |db| db.get_current_persona_id(canonical_chat_id)).await.unwrap_or(0);
                    if pid == 0 {
                        let _ = msg.channel_id.say(&ctx.http, "No session to archive.").await;
                    } else {
                        let pid_f = pid;
                        if let Ok(Some((json, _))) = call_blocking(self.app_state.db.clone(), move |db| {
                            db.load_session(canonical_chat_id, pid_f)
                        })
                        .await
                        {
                            let messages: Vec<ClaudeMessage> = serde_json::from_str(&json).unwrap_or_default();
                            if messages.is_empty() {
                                let _ = msg.channel_id.say(&ctx.http, "No session to archive.").await;
                            } else {
                                archive_conversation(&self.app_state.config.runtime_data_dir(), canonical_chat_id, &messages);
                                let _ = msg
                                    .channel_id
                                    .say(&ctx.http, format!("Archived {} messages.", messages.len()))
                                    .await;
                            }
                        } else {
                            let _ = msg.channel_id.say(&ctx.http, "No session to archive.").await;
                        }
                    }
                }
            }
            return;
        }

        if text.trim().is_empty() && image_data.is_none() {
            return;
        }

        // Resolve persona for this contact
        let persona_id = call_blocking(self.app_state.db.clone(), move |db| db.get_current_persona_id(canonical_chat_id)).await.unwrap_or(0);
        if persona_id == 0 {
            return;
        }

        // Store the chat and message
        let title = format!("discord-{}", msg.channel_id.get());
        let _ = call_blocking(self.app_state.db.clone(), move |db| {
            db.upsert_chat(canonical_chat_id, Some(&title), "discord")
        })
        .await;

        let stored = StoredMessage {
            id: msg.id.get().to_string(),
            chat_id: canonical_chat_id,
            persona_id,
            sender_name: sender_name.clone(),
            content: text.clone(),
            is_from_bot: false,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        let _ = call_blocking(self.app_state.db.clone(), move |db| {
            db.store_message(&stored)
        })
        .await;

        // Determine if we should respond
        let should_respond = if msg.guild_id.is_some() {
            // In a guild: only respond to @mentions
            let cache = &ctx.cache;
            let bot_id = cache.current_user().id;
            msg.mentions.iter().any(|u| u.id == bot_id)
        } else {
            // DM: respond to all messages
            true
        };

        if !should_respond {
            return;
        }

        info!(
            "Discord message from {} in channel {}: {}",
            sender_name,
            channel_id,
            text.chars().take(100).collect::<String>()
        );

        // Start typing indicator
        let typing = msg.channel_id.start_typing(&ctx.http);

        // Process with Claude (reuses the same agentic loop as Telegram)
        match crate::telegram::process_with_agent(
            &self.app_state,
            AgentRequestContext {
                caller_channel: "discord",
                chat_id: canonical_chat_id,
                chat_type: if msg.guild_id.is_some() {
                    "group"
                } else {
                    "private"
                },
                persona_id,
                is_scheduled_task: false,
            },
            None,
            image_data,
        )
        .await
        {
            Ok(response) => {
                drop(typing);
                if !response.is_empty() {
                    if let Err(e) = crate::channel::deliver_to_contact(
                        self.app_state.db.clone(),
                        Some(&self.app_state.bot),
                        self.app_state.discord_http.as_deref(),
                        &self.app_state.config.bot_username,
                        canonical_chat_id,
                        persona_id,
                        &response,
                    )
                    .await
                    {
                        tracing::warn!(target: "channel", error = %e, "deliver_to_contact failed; sending to Discord only");
                        send_discord_response(&ctx, msg.channel_id, &response).await;
                    }
                }
            }
            Err(e) => {
                drop(typing);
                error!("Error processing Discord message: {e}");
                let _ = msg.channel_id.say(&ctx.http, format!("Error: {e}")).await;
            }
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);
    }
}

fn sanitize_upload_filename(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '_',
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "discord-upload.bin".to_string()
    } else {
        sanitized
    }
}

/// Split and send long messages (Discord limit is 2000 chars).
async fn send_discord_response(ctx: &Context, channel_id: ChannelId, text: &str) {
    const MAX_LEN: usize = 2000;

    if text.len() <= MAX_LEN {
        let _ = channel_id.say(&ctx.http, text).await;
        return;
    }

    let mut remaining = text;
    while !remaining.is_empty() {
        let chunk_len = if remaining.len() <= MAX_LEN {
            remaining.len()
        } else {
            remaining[..MAX_LEN].rfind('\n').unwrap_or(MAX_LEN)
        };

        let chunk = &remaining[..chunk_len];
        let _ = channel_id.say(&ctx.http, chunk).await;
        remaining = &remaining[chunk_len..];

        if remaining.starts_with('\n') {
            remaining = &remaining[1..];
        }
    }
}

/// Start the Discord bot. Called from run_bot() if discord_bot_token is configured.
pub async fn start_discord_bot(app_state: Arc<AppState>, token: &str) {
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler { app_state };

    let mut client = match Client::builder(token, intents).event_handler(handler).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create Discord client: {e}");
            return;
        }
    };

    info!("Starting Discord bot...");
    if let Err(e) = client.start().await {
        error!("Discord bot error: {e}");
    }
}
