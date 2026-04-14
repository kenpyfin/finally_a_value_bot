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
