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
}

export type ChannelBinding = {
  channel_type: string
  channel_handle: string
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

export type BackgroundJob = {
  id: string
  chat_id: number
  persona_id: number
  prompt: string
  status: string // "pending" | "running" | "completed_raw" | "main_agent_processing" | "done" | "failed"
  trigger_reason: string
  created_at: string
  started_at: string | null
  finished_at: string | null
  result_preview: string | null
  error_text: string | null
}

export type MessageItem = {
  id: string
  sender_name: string
  content: string
  is_from_bot: boolean
  timestamp: string
}
