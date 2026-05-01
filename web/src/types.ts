export type SessionItem = {
  session_key: string
  label: string
  chat_id: number
  chat_type: string
  last_message_time?: string
  last_message_preview?: string | null
}

/** Alias for SessionItem; contact = one conversation (unified across channels). */
export type ContactItem = SessionItem

export type Persona = {
  id: number
  name: string
  is_active: boolean
  last_bot_message_at?: string | null
}

export type ChannelBinding = {
  bot_instance_id: number
  channel_type: string
  channel_handle: string
  persona_mode?: 'all' | 'single'
  persona_id?: number | null
}

export type RuntimeSettingItem = {
  key: string
  value: string
  raw_value: string
  is_secret: boolean
  updated_at?: string
  source?: string
}

/** Matches `/api/settings` `installation_status` and web Settings UI. */
export type InstallationStatus = {
  llm_ready: boolean
  channel_ready: boolean
  web_enabled: boolean
  /** @deprecated use requires_restart_for_env_changes */
  requires_restart_to_apply_runtime_settings?: boolean
  requires_restart_for_env_changes?: boolean
  runtime_env_merge_from_app_settings?: boolean
}

/** Redacted row from `GET /api/channel_bot_instances`. */
export type BotInstanceRow = {
  id: number
  platform: string
  label: string
  token_redacted: string
  created_at: string
  env_primary: boolean
}

export type ScheduleTask = {
  id: number
  chat_id: number
  persona_id: number
  prompt: string
  schedule_type: string
  schedule_value: string
  next_run: string | null
  last_run: string | null
  status: string
  created_at: string | null
}

export type MessageItem = {
  id: string
  sender_name: string
  content: string
  is_from_bot: boolean
  timestamp: string
}

export type ArtifactItem = {
  id: string
  name: string
  kind: string
  size_bytes?: number | null
  created_at?: string | null
  source: string
  url: string
  preview_url: string
}

/** Message row from `GET /api/history`. */
export type BackendMessage = {
  id?: string
  sender_name?: string
  content?: string
  is_from_bot?: boolean
  timestamp?: string
  is_bookmarked?: boolean
}

export type PersonaBulletinFocus = {
  title?: string | null
  content: string
  updated_at: string
}

export type PersonaMessageBookmark = {
  message_id: string
  role: 'user' | 'assistant' | string
  content_preview: string
  note?: string | null
  created_at?: string
  updated_at?: string
}

export type QueueItem = {
  run_id: string
  persona_id: number
  persona_name: string
  source: string
  label: string
  state: string
  project_id?: number | null
  workflow_id?: number | null
  position: number
}

export type QueueLane = {
  chat_id: number
  pending: number
  active_for_ms: number
  oldest_wait_ms: number
  last_error?: string | null
  project_id?: number | null
  workflow_id?: number | null
  items?: QueueItem[]
}

export type BackgroundJobHeartbeat = {
  run_key: string
  chat_id: number
  persona_id: number
  job_type: string
  stage: string
  message: string
  active: boolean
  updated_at: string
}

export type BackgroundJobItem = {
  id: string
  chat_id: number
  persona_id: number
  prompt: string
  status: string
  trigger_reason: string
  created_at: string
  started_at?: string | null
  finished_at?: string | null
  result_preview?: string | null
  error_text?: string | null
  heartbeat?: BackgroundJobHeartbeat | null
}
