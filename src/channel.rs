use std::path::PathBuf;
use std::sync::Arc;

use teloxide::prelude::*;

use crate::channels::telegram::send_response_result;
use crate::db::{call_blocking, Database, StoredMessage};
use crate::tools::auth_context_from_input;

pub async fn is_web_chat(db: Arc<Database>, chat_id: i64) -> bool {
    matches!(
        call_blocking(db, move |d| d.get_chat_type(chat_id)).await,
        Ok(Some(ref t)) if t == "web"
    )
}

pub async fn enforce_channel_policy(
    db: Arc<Database>,
    input: &serde_json::Value,
    target_chat_id: i64,
) -> Result<(), String> {
    let Some(auth) = auth_context_from_input(input) else {
        return Ok(());
    };

    if is_web_chat(db, auth.caller_chat_id).await && auth.caller_chat_id != target_chat_id {
        return Err("Permission denied: web chats cannot operate on other chats".into());
    }

    Ok(())
}

pub async fn deliver_and_store_bot_message(
    bot: &Bot,
    db: Arc<Database>,
    bot_username: &str,
    chat_id: i64,
    persona_id: i64,
    text: &str,
    workspace_root: Option<PathBuf>,
) -> Result<(), String> {
    if is_web_chat(db.clone(), chat_id).await {
        let msg = StoredMessage {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id,
            persona_id,
            sender_name: bot_username.to_string(),
            content: text.to_string(),
            is_from_bot: true,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        call_blocking(db.clone(), move |d| d.store_message(&msg))
            .await
            .map_err(|e| format!("Failed to store web message: {e}"))
    } else {
        let send_result =
            send_response_result(bot, ChatId(chat_id), text, None, workspace_root.as_deref()).await;
        let msg = StoredMessage {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id,
            persona_id,
            sender_name: bot_username.to_string(),
            content: text.to_string(),
            is_from_bot: true,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        match &send_result {
            Ok(_) => {}
            Err(e) => {
                let err_str = e.to_string();
                // Chat may have been deleted or bot removed; still store so conversation history is intact (e.g. web UI can show reply).
                if err_str.contains("chat not found")
                    || err_str.contains("Chat not found")
                    || err_str.contains("user is deactivated")
                {
                    tracing::warn!(
                        target: "channel",
                        chat_id = chat_id,
                        error = %err_str,
                        "Telegram delivery failed (chat unavailable); storing message anyway"
                    );
                    call_blocking(db.clone(), move |d| d.store_message(&msg))
                        .await
                        .map_err(|e| format!("Failed to store message: {e}"))?;
                    return Ok(());
                }

                return Err(format!("Failed to send message: {e}"));
            }
        }
        call_blocking(db.clone(), move |d| d.store_message(&msg))
            .await
            .map_err(|e| format!("Failed to store sent message: {e}"))
    }
}

/// Store the bot message once under canonical_chat_id and deliver to all bound channels (Telegram, Discord, web).
/// Used for unified contact sync: the same reply appears on every linked channel.
pub async fn deliver_to_contact(
    db: Arc<Database>,
    bot: Option<&Bot>,
    discord_http: Option<&serenity::http::Http>,
    bot_username: &str,
    canonical_chat_id: i64,
    persona_id: i64,
    text: &str,
    workspace_root: Option<PathBuf>,
) -> Result<(), String> {
    let msg = StoredMessage {
        id: uuid::Uuid::new_v4().to_string(),
        chat_id: canonical_chat_id,
        persona_id,
        sender_name: bot_username.to_string(),
        content: text.to_string(),
        is_from_bot: true,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    call_blocking(db.clone(), move |d| d.store_message(&msg))
        .await
        .map_err(|e| format!("Failed to store message: {e}"))?;

    let bindings = call_blocking(db.clone(), move |d| d.list_bindings_for_contact(canonical_chat_id))
        .await
        .map_err(|e| format!("Failed to list bindings: {e}"))?;

    for b in &bindings {
        match b.channel_type.as_str() {
            "telegram" => {
                if let Some(bot) = bot {
                    if let Ok(chat_id) = b.channel_handle.parse::<i64>() {
                        if let Err(e) =
                            send_response_result(bot, ChatId(chat_id), text, None, workspace_root.as_deref())
                                .await
                        {
                            let err_str = e.to_string();
                            if !err_str.contains("chat not found")
                                && !err_str.contains("Chat not found")
                                && !err_str.contains("user is deactivated")
                            {
                                tracing::warn!(target: "channel", chat_id = chat_id, error = %err_str, "Telegram delivery to bound channel failed");
                            }
                        }
                    }
                }
            }
            "discord" => {
                if let Some(http) = discord_http {
                    if let Ok(channel_id_u64) = b.channel_handle.parse::<u64>() {
                        let channel_id = serenity::model::id::ChannelId::new(channel_id_u64);
                        const MAX_LEN: usize = 2000;
                        let content = text.to_string();
                        if content.len() <= MAX_LEN {
                            if let Err(e) = channel_id.say(http, &content).await {
                                tracing::warn!(target: "channel", channel_id = %channel_id_u64, error = %e, "Discord delivery to bound channel failed");
                            }
                        } else {
                            let chars: Vec<char> = content.chars().collect();
                            for chunk in chars.chunks(MAX_LEN) {
                                let s: String = chunk.iter().collect();
                                let _ = channel_id.say(http, &s).await;
                            }
                        }
                    }
                }
            }
            "web" => {
                // Already stored above; web clients load from history or SSE
            }
            _ => {}
        }
    }

    Ok(())
}
