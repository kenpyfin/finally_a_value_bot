use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use teloxide::prelude::*;

use crate::channels::telegram::{
    send_response_result, strip_embedded_bulletin_focus, WorkspaceAutoImageContext,
};
use crate::db::{call_blocking, message_origin_interactive, Database, StoredMessage};
use crate::final_delivery_dedupe::{plan_agent_final_delivery, AgentFinalDeliveryPlan};
use crate::final_delivery_media::normalize_assistant_artifact_references;
use crate::tools::auth_context_from_input;

/// How a stored outbound message should be classified in `messages.origin`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MessageStoreOrigin {
    #[default]
    Interactive,
    Scheduled,
}

impl MessageStoreOrigin {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Interactive => crate::db::MESSAGE_ORIGIN_INTERACTIVE,
            Self::Scheduled => crate::db::MESSAGE_ORIGIN_SCHEDULED,
        }
    }

    fn into_origin_string(self) -> String {
        self.as_db_str().to_string()
    }
}

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
        return Err("Permission denied: web UI sessions cannot operate on other chats".into());
    }

    Ok(())
}

fn strip_leading_persona_tokens(text: &str) -> &str {
    let mut rest = text.trim_start();
    loop {
        if !rest.starts_with('[') {
            break;
        }
        let Some(close_idx) = rest.find(']') else {
            break;
        };
        // Only treat short single-line bracket heads as transport persona tags.
        let token = &rest[1..close_idx];
        if token.is_empty() || token.len() > 64 || token.contains('\n') {
            break;
        }
        rest = rest[close_idx + 1..].trim_start();
    }
    rest
}

fn normalize_persona_prefixed_text(persona_name: &str, text: &str) -> String {
    let body = strip_leading_persona_tokens(text).trim();
    if body.is_empty() {
        format!("[{persona_name}]")
    } else {
        format!("[{persona_name}] {body}")
    }
}

/// Prepend `[PersonaName] ` to outbound bot text so users know which persona sent it.
pub async fn with_persona_indicator(db: Arc<Database>, persona_id: i64, text: &str) -> String {
    let name = match call_blocking(db, move |d| d.get_persona(persona_id)).await {
        Ok(Some(p)) => p.name,
        _ => "Unknown".to_string(),
    };
    normalize_persona_prefixed_text(&name, text)
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
    let text = strip_embedded_bulletin_focus(text);
    let text = crate::agent_turn_context::strip_stored_dialogue_markup(&text);
    let text = &with_persona_indicator(db.clone(), persona_id, &text).await;
    if is_web_chat(db.clone(), chat_id).await {
        let msg = StoredMessage {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id,
            persona_id,
            session_id: None,
            sender_name: bot_username.to_string(),
            content: text.to_string(),
            is_from_bot: true,
            timestamp: chrono::Utc::now().to_rfc3339(),
            origin: message_origin_interactive(),
        };
        call_blocking(db.clone(), move |d| d.store_message(&msg))
            .await
            .map_err(|e| format!("Failed to store web message: {e}"))
    } else {
        let auto_images = workspace_root
            .as_ref()
            .map(|root| WorkspaceAutoImageContext {
                root,
                chat_id,
                persona_id,
            });
        let send_result = send_response_result(bot, ChatId(chat_id), text, None, auto_images).await;
        let msg = StoredMessage {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id,
            persona_id,
            session_id: None,
            sender_name: bot_username.to_string(),
            content: text.to_string(),
            is_from_bot: true,
            timestamp: chrono::Utc::now().to_rfc3339(),
            origin: message_origin_interactive(),
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

/// Controls which external bindings receive an outbound message (web history always stored).
#[derive(Debug, Clone, Copy, Default)]
pub enum DeliveryScope {
    /// Deliver to every bound channel for this contact (scheduler, web-initiated, background jobs).
    #[default]
    ContactWide,
    /// Reply only on the platform + bot instance that received the inbound message.
    PlatformInstance {
        channel_type: &'static str,
        bot_instance_id: i64,
    },
    /// Persist for web/history only; no Telegram/Discord/WhatsApp send.
    StoreOnly,
}

/// Store the bot message once under canonical_chat_id and deliver per [`DeliveryScope`].
pub async fn deliver_to_contact(
    db: Arc<Database>,
    telegram_bots: &HashMap<i64, Bot>,
    discord_http: &HashMap<i64, Arc<serenity::http::Http>>,
    bot_username: &str,
    canonical_chat_id: i64,
    persona_id: i64,
    text: &str,
    workspace_root: Option<PathBuf>,
    scope: DeliveryScope,
    session_id: Option<String>,
) -> Result<(), String> {
    deliver_to_contact_with_origin(
        db,
        telegram_bots,
        discord_http,
        bot_username,
        canonical_chat_id,
        persona_id,
        text,
        workspace_root,
        scope,
        session_id,
        MessageStoreOrigin::Interactive,
    )
    .await
}

pub async fn deliver_to_contact_with_origin(
    db: Arc<Database>,
    telegram_bots: &HashMap<i64, Bot>,
    discord_http: &HashMap<i64, Arc<serenity::http::Http>>,
    bot_username: &str,
    canonical_chat_id: i64,
    persona_id: i64,
    text: &str,
    workspace_root: Option<PathBuf>,
    scope: DeliveryScope,
    session_id: Option<String>,
    message_origin: MessageStoreOrigin,
) -> Result<(), String> {
    let text = strip_embedded_bulletin_focus(text);
    let text = crate::agent_turn_context::strip_stored_dialogue_markup(&text);
    let text = &with_persona_indicator(db.clone(), persona_id, &text).await;
    let msg = StoredMessage {
        id: uuid::Uuid::new_v4().to_string(),
        chat_id: canonical_chat_id,
        persona_id,
        session_id,
        sender_name: bot_username.to_string(),
        content: text.to_string(),
        is_from_bot: true,
        timestamp: chrono::Utc::now().to_rfc3339(),
        origin: message_origin.into_origin_string(),
    };
    call_blocking(db.clone(), move |d| d.store_message(&msg))
        .await
        .map_err(|e| format!("Failed to store message: {e}"))?;

    let bindings = call_blocking(db.clone(), move |d| {
        d.list_bindings_for_contact(canonical_chat_id)
    })
    .await
    .map_err(|e| format!("Failed to list bindings: {e}"))?;
    let policies = call_blocking(db.clone(), move |d| {
        d.list_channel_persona_policies(canonical_chat_id)
    })
    .await
    .map_err(|e| format!("Failed to list channel persona policies: {e}"))?;
    let mut policy_by_instance: std::collections::HashMap<
        i64,
        (crate::db::ChannelPersonaMode, Option<i64>),
    > = std::collections::HashMap::new();
    for p in policies {
        policy_by_instance.insert(p.bot_instance_id, (p.mode, p.persona_id));
    }

    let mut delivered_targets: HashSet<(String, String)> = HashSet::new();
    if matches!(scope, DeliveryScope::StoreOnly) {
        return Ok(());
    }
    for b in &bindings {
        if let DeliveryScope::PlatformInstance {
            channel_type,
            bot_instance_id,
        } = scope
        {
            if b.channel_type != channel_type || b.bot_instance_id != bot_instance_id {
                continue;
            }
        }
        if let Some((mode, policy_persona_id)) = policy_by_instance.get(&b.bot_instance_id) {
            if *mode == crate::db::ChannelPersonaMode::Single
                && policy_persona_id.is_some()
                && *policy_persona_id != Some(persona_id)
            {
                continue;
            }
        }
        let target_key = (b.channel_type.clone(), b.channel_handle.clone());
        if !delivered_targets.insert(target_key) {
            continue;
        }
        match b.channel_type.as_str() {
            "telegram" => {
                let tg_bot = match scope {
                    DeliveryScope::PlatformInstance { .. } => telegram_bots.get(&b.bot_instance_id),
                    _ => telegram_bots
                        .get(&b.bot_instance_id)
                        .or_else(|| telegram_bots.get(&crate::db::BOT_INSTANCE_TELEGRAM_PRIMARY)),
                };
                if let Some(bot) = tg_bot {
                    if let Ok(chat_id) = b.channel_handle.parse::<i64>() {
                        let auto_images =
                            workspace_root
                                .as_ref()
                                .map(|root| WorkspaceAutoImageContext {
                                    root,
                                    chat_id: canonical_chat_id,
                                    persona_id,
                                });
                        if let Err(e) =
                            send_response_result(bot, ChatId(chat_id), text, None, auto_images)
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
                let http = match scope {
                    DeliveryScope::PlatformInstance { .. } => discord_http.get(&b.bot_instance_id),
                    _ => discord_http
                        .get(&b.bot_instance_id)
                        .or_else(|| discord_http.get(&crate::db::BOT_INSTANCE_DISCORD_PRIMARY)),
                };
                if let Some(http) = http {
                    if let Ok(channel_id_u64) = b.channel_handle.parse::<u64>() {
                        let channel_id = serenity::model::id::ChannelId::new(channel_id_u64);
                        const MAX_LEN: usize = 2000;
                        let content = text.to_string();
                        if content.len() <= MAX_LEN {
                            if let Err(e) = channel_id.say(http.as_ref(), &content).await {
                                tracing::warn!(target: "channel", channel_id = %channel_id_u64, error = %e, "Discord delivery to bound channel failed");
                            }
                        } else {
                            let chars: Vec<char> = content.chars().collect();
                            for chunk in chars.chunks(MAX_LEN) {
                                let s: String = chunk.iter().collect();
                                let _ = channel_id.say(http.as_ref(), &s).await;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFinalDeliveryOutcome {
    /// Text the HTTP API should echo (`""` when the final was suppressed as redundant).
    pub response_for_client: String,
}

/// Agent loop completion only: plan delivery (empty-body skip) then [`deliver_to_contact`].
fn normalize_final_for_delivery(
    raw_final: &str,
    workspace_root: Option<&Path>,
    canonical_chat_id: i64,
    persona_id: i64,
) -> String {
    let cleaned = crate::agent_turn_context::strip_stored_dialogue_markup(
        &strip_embedded_bulletin_focus(raw_final),
    );
    match workspace_root {
        Some(root) => {
            normalize_assistant_artifact_references(&cleaned, root, canonical_chat_id, persona_id)
        }
        None => cleaned,
    }
}

pub async fn deliver_agent_final_to_contact(
    db: Arc<Database>,
    telegram_bots: &HashMap<i64, Bot>,
    discord_http: &HashMap<i64, Arc<serenity::http::Http>>,
    bot_username: &str,
    canonical_chat_id: i64,
    persona_id: i64,
    raw_final: &str,
    workspace_root: Option<PathBuf>,
    scope: DeliveryScope,
    session_id: Option<String>,
) -> Result<AgentFinalDeliveryOutcome, String> {
    deliver_agent_final_to_contact_with_origin(
        db,
        telegram_bots,
        discord_http,
        bot_username,
        canonical_chat_id,
        persona_id,
        raw_final,
        workspace_root,
        scope,
        session_id,
        MessageStoreOrigin::Interactive,
    )
    .await
}

pub async fn deliver_agent_final_to_contact_with_origin(
    db: Arc<Database>,
    telegram_bots: &HashMap<i64, Bot>,
    discord_http: &HashMap<i64, Arc<serenity::http::Http>>,
    bot_username: &str,
    canonical_chat_id: i64,
    persona_id: i64,
    raw_final: &str,
    workspace_root: Option<PathBuf>,
    scope: DeliveryScope,
    session_id: Option<String>,
    message_origin: MessageStoreOrigin,
) -> Result<AgentFinalDeliveryOutcome, String> {
    let cleaned = normalize_final_for_delivery(
        raw_final,
        workspace_root.as_deref(),
        canonical_chat_id,
        persona_id,
    );
    let indicated = with_persona_indicator(db.clone(), persona_id, &cleaned).await;
    let plan = plan_agent_final_delivery(None, &indicated);

    match plan {
        AgentFinalDeliveryPlan::DeliverFull => {
            deliver_to_contact_with_origin(
                db.clone(),
                telegram_bots,
                discord_http,
                bot_username,
                canonical_chat_id,
                persona_id,
                &cleaned,
                workspace_root,
                scope,
                session_id,
                message_origin,
            )
            .await?;
            Ok(AgentFinalDeliveryOutcome {
                response_for_client: cleaned,
            })
        }
        AgentFinalDeliveryPlan::DeliverSuffixOnly(suffix) => {
            let suffix = normalize_final_for_delivery(
                &suffix,
                workspace_root.as_deref(),
                canonical_chat_id,
                persona_id,
            );
            deliver_to_contact_with_origin(
                db.clone(),
                telegram_bots,
                discord_http,
                bot_username,
                canonical_chat_id,
                persona_id,
                &suffix,
                workspace_root,
                scope,
                session_id,
                message_origin,
            )
            .await?;
            Ok(AgentFinalDeliveryOutcome {
                response_for_client: suffix,
            })
        }
        AgentFinalDeliveryPlan::Skip => {
            tracing::info!(
                target: "channel",
                chat_id = canonical_chat_id,
                "Skipping agent final delivery (empty body)"
            );
            Ok(AgentFinalDeliveryOutcome {
                response_for_client: String::new(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_persona_prefixed_text;

    #[test]
    fn persona_prefix_is_added_once() {
        let out = normalize_persona_prefixed_text("InfluencerPZ", "Hello");
        assert_eq!(out, "[InfluencerPZ] Hello");
    }

    #[test]
    fn repeated_leading_persona_tags_are_collapsed() {
        let out = normalize_persona_prefixed_text(
            "InfluencerPZ",
            "[InfluencerPZ] [InfluencerPZ] [InfluencerPZ] Hi there",
        );
        assert_eq!(out, "[InfluencerPZ] Hi there");
    }

    #[test]
    fn other_persona_tag_is_replaced_with_current() {
        let out = normalize_persona_prefixed_text("Trader", "[InfluencerPZ] Market open");
        assert_eq!(out, "[Trader] Market open");
    }
}
