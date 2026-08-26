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
  /** Session that produced the latest bot message; null/undefined = main chat. */
  last_bot_message_session_id?: string | null
  last_bot_message_session_title?: string | null
  agent_engine_override?: string | null
  agent_engine_effective?: string
}

export type ChannelBinding = {
  bot_instance_id: number
  channel_type: string
  /** Telegram chat id or Discord channel id; null when not linked yet. */
  channel_handle: string | null
  platform?: string
  label?: string
  linked?: boolean
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
export type TerminalCapabilities = {
  web_terminal_enabled?: boolean
  web_terminal_available?: boolean
  web_terminal_blocked_reason?: string | null
  web_terminal_max_sessions?: number
  web_terminal_idle_timeout_secs?: number
}

/** Matches `/api/settings` `installation_status` and web Settings UI. */
export type InstallationStatus = {
  llm_ready: boolean
  channel_ready: boolean
  cursor_engine_ready?: boolean
  local_delegate_ready?: boolean
  agent_engine?: string
  cost_routing_effective?: boolean
  web_enabled: boolean
  terminal?: TerminalCapabilities
  /** @deprecated use requires_restart_for_env_changes */
  requires_restart_to_apply_runtime_settings?: boolean
  requires_restart_for_env_changes?: boolean
  runtime_env_merge_from_app_settings?: boolean
  llm_model_from_app_settings?: boolean
}

export type LlmCatalogModel = {
  id: string
  input_usd_per_mtok?: number | null
  output_usd_per_mtok?: number | null
  cost_tier: string
  cost_summary: string
  from_active_config?: boolean
  from_live?: boolean
}

export type CursorModelParam = {
  id: string
  value: string
}

export type CursorModelParameterValue = {
  value: string
  display_name?: string
}

export type CursorModelParameterDef = {
  id: string
  display_name?: string
  values: CursorModelParameterValue[]
}

export type CursorModelVariant = {
  params: CursorModelParam[]
  display_name: string
  description?: string
  is_default?: boolean
}

export type CursorModelCatalogEntry = {
  id: string
  display_name?: string
  parameters?: CursorModelParameterDef[]
  variants?: CursorModelVariant[]
}

export type CursorEngineConfigResponse = {
  ok?: boolean
  sdk_runner_url?: string
  sdk_model?: string
  sdk_model_params?: CursorModelParam[]
  sdk_runner_ok?: boolean
  sidecar_managed?: boolean
  sidecar_reachable?: boolean
  api_key_configured?: boolean
  engine_ready?: boolean
  agent_engine?: 'classic' | 'deterministic' | 'cursor'
  cli_path?: string
  cli_model?: string
  cli_runner_url?: string
  cli_on_path?: boolean
  timeout_secs?: number
  tmux_enabled?: boolean
  mcp_tools_enabled?: boolean
  mcp_expose_send_message?: boolean
  delegation_slim_prompt?: boolean
  delegation_resume_delta?: boolean
  mcp_endpoint_url?: string
  mcp_bridge_ready?: boolean
  install_steps?: string[]
  sidecar_error?: string | null
  health_ok?: boolean
  message?: string
}

export type RuntimeConfigResponse = {
  ok?: boolean
  tool_output_debug?: boolean
  post_tool_evaluator_enabled?: boolean
  response_quality_evaluator_enabled?: boolean
  agent_engine?: 'classic' | 'classic_cost_routing' | 'deterministic' | 'cursor'
  local_delegate_configured?: boolean
  local_delegate_tools_ok?: boolean
  local_delegate_ready?: boolean
  cost_routing_effective?: boolean
  warnings?: string[]
  source?: 'env' | 'app_settings'
  sources?: {
    tool_output_debug?: 'env' | 'app_settings'
    post_tool_evaluator_enabled?: 'env' | 'app_settings'
    response_quality_evaluator_enabled?: 'env' | 'app_settings'
    agent_engine?: 'env' | 'app_settings'
  }
  description?: string
  message?: string
}

export type HookDefinition = {
  id: number
  name: string
  event_name: string
  matcher?: string | null
  action_type: string
  action_payload_json: string
  action_payload?: Record<string, unknown>
  scoped_persona_ids: number[] | null
  is_global: boolean
  enabled: boolean
  updated_at: string
  scoped_for_persona?: boolean
  allowed_for_persona?: boolean
  active_for_persona?: boolean
}

export type SkillCatalogEntry = {
  name: string
  description: string
  when_to_use?: string
  platforms?: string[]
  deps?: string[]
  source?: string
  version?: string
  updated_at?: string
  remote?: boolean
  allowed_for_persona?: boolean
}

export type PipelinePhaseKind =
  | 'intent_classify'
  | 'plan_generate'
  | 'execute_plan'
  | 'synthesize_delivery'

export type PipelineModelRoute =
  | 'inherit_global'
  | 'strategy'
  | 'local'

export type PipelineTransitionCondition =
  | 'always'
  | 'intent_category_conversational'
  | 'intent_category_question'
  | 'intent_category_task'
  | 'intent_needs_clarification'
  | 'intent_needs_clarification_proceed'
  | 'plan_empty'
  | 'execute_any_failed'
  | 'execute_all_succeeded'
  | 'channel_web'
  | 'is_scheduled'

export type PipelineTransitionTarget =
  | { direct_answer: true }
  | { clarify: true }
  | { finish: true }
  | { phase: string }

export type PipelineTransitionRule = {
  when: PipelineTransitionCondition
  goto:
    | 'direct_answer'
    | 'clarify'
    | 'finish'
    | { phase: string }
}

export type PipelineOperationalConfig = {
  timeout_secs: number
  max_iterations: number
  max_iterations_local: number
  max_plan_steps: number
  llm_round_timeout_secs: number
  tool_execution_timeout_secs: number
  iteration_breaker_min_chars: number
  compact_system_max_chars: number
  collapsed_session_turns: number
  sop_reference_max_chars: number
  min_polish_only_summary_chars: number
  max_polish_only_combined_chars: number
}

export type PipelinePolicyConfig = {
  heuristic_intent_enabled: boolean
  merged_classify_and_plan_enabled: boolean
  skip_consolidate_when_good: boolean
  clarify_on_web_proceed: boolean
  clarify_on_scheduler_proceed: boolean
  image_input_force_task: boolean
  retry_failed_steps: boolean
  escalate_to_strategy_on_skill_failure: boolean
  use_local_for_json_stages: boolean
  bind_persona_sops_in_plan: boolean
}

export type PriorStepFeedMode = 'full' | 'summary'

export type PhaseContextIncludes = {
  include_system_prompt: boolean
  include_agent_system_prompt: boolean
  include_skills_catalog: boolean
  include_session_excerpt: boolean
  include_persona_memory: boolean
  include_workspace_paths: boolean
  include_sop_reference: boolean
  include_current_request: boolean
  include_prior_step_summaries: boolean
  prior_step_feed_mode: PriorStepFeedMode
  prior_step_summary_prompt: string
  prior_step_full_output_max_chars: number
  include_step_contract: boolean
  include_execution_summary: boolean
}

export type PipelinePhase = {
  id: string
  label: string
  enabled: boolean
  kind: PipelinePhaseKind
  model_route: PipelineModelRoute
  system_prompt: string
  preamble?: string | null
  context_includes: PhaseContextIncludes
  transitions: PipelineTransitionRule[]
}

export type PipelineProfile = {
  version: number
  entry_phase_id: string
  phases: PipelinePhase[]
  operational: PipelineOperationalConfig
  policies: PipelinePolicyConfig
}

export type DeterministicPipelineResponse = {
  ok?: boolean
  schema_version?: number
  profile?: PipelineProfile
  defaults?: PipelineProfile
  builtin_prompts?: Record<string, string>
  agent_engine?: 'classic' | 'deterministic' | 'cursor'
  message?: string
}

export type PersonaHookSkillPolicy = {
  allowed_hook_ids: number[] | null
  allowed_skill_names: string[] | null
  uses_default_hooks: boolean
  uses_default_skills: boolean
  updated_at?: string | null
}

export type LlmProviderOption = {
  id: string
  label: string
  api_key_configured: boolean
  api_key_env_hints: string[]
  default_base_url?: string | null
  models: LlmCatalogModel[]
}

export type LlmConfigResponse = {
  ok?: boolean
  provider?: { id: string; label: string }
  provider_source?: 'app_settings' | 'default'
  api_key_configured?: boolean
  model?: string
  model_in_catalog?: boolean
  model_source?: 'app_settings' | 'default'
  is_local_provider?: boolean
  base_url?: string | null
  default_base_url?: string | null
  base_url_source?: 'app_settings' | 'default' | 'n/a'
  catalog?: LlmCatalogModel[]
  providers?: LlmProviderOption[]
  catalog_source?: 'static_curated' | 'live' | 'static_fallback'
  cost_reference_note?: string
  custom_model_allowed?: boolean
  thinking_enabled?: boolean
  thinking_source?: 'app_settings' | 'default'
  thinking_supported?: boolean
  show_thinking?: boolean
  show_thinking_source?: 'app_settings' | 'env' | 'default'
}

export type LlmLiveCatalogResponse = {
  ok?: boolean
  provider?: string
  source?: 'live' | 'static_fallback'
  truncated?: boolean
  live_count?: number | null
  models?: LlmCatalogModel[]
  base_url?: string | null
  message?: string | null
}

export type LocalDelegateConfigResponse = {
  ok?: boolean
  routing_enabled?: boolean
  enabled?: boolean
  local_base_url?: string
  local_model?: string
  local_tools_ok?: boolean
  tier1_base_url?: string
  tier1_model?: string
  tier2_base_url?: string
  tier2_model?: string
  tier1_tools_ok?: boolean
  tier2_tools_ok?: boolean
  strategy_provider?: string
  strategy_model?: string
  description?: string
}

/** @deprecated use LocalDelegateConfigResponse */
export type MultimodelConfigResponse = LocalDelegateConfigResponse

export type AgentHistoryOptimizeResponse = {
  ok?: boolean
  job_id?: string
  filename?: string
  message?: string
}

export type AgentHistoryOptimizeRequest = {
  operator_notes?: string
}

/** Redacted row from `GET /api/channel_bot_instances`. */
export type BotInstanceRow = {
  id: number
  platform: 'telegram' | 'discord' | 'whatsapp' | string
  label: string
  token_set?: boolean
  token_redacted: string
  bot_username?: string
  allowed_groups?: string
  discord_allowed_channels?: string
  whatsapp_phone_number_id?: string
  whatsapp_verify_token_set?: boolean
  whatsapp_verify_token_redacted?: string
  whatsapp_webhook_port?: number
  wecom_corp_id?: string
  wecom_agent_id?: number
  wecom_callback_token_set?: boolean
  wecom_callback_token_redacted?: string
  wecom_encoding_aes_key_set?: boolean
  wecom_encoding_aes_key_redacted?: string
  wecom_webhook_port?: number
  wecom_allowed_chats?: string
  wecom_aibot_id?: string
  wecom_mode?: string
  created_at: string
  env_primary?: boolean
  is_primary?: boolean
}

/** Response from `GET/PATCH /api/channels/integration`. */
export type ChannelIntegrationSettings = {
  ok?: boolean
  message?: string
  bot_username: string
  allowed_groups: string
  control_chat_ids: string
  discord_allowed_channels: string
  whatsapp_phone_number_id: string
  whatsapp_verify_token_set: boolean
  whatsapp_verify_token_redacted: string
  whatsapp_webhook_port: number
  telegram_token_set: boolean
  telegram_token_redacted: string
  telegram_label: string
  discord_token_set: boolean
  discord_token_redacted: string
  discord_label: string
  whatsapp_access_token_set: boolean
  whatsapp_access_token_redacted: string
  whatsapp_label: string
  instances?: BotInstanceRow[]
  requires_restart?: boolean
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

export type PersonaTodo = {
  id: number
  chat_id: number
  persona_id: number
  title: string
  status: string
  source_hint?: string | null
  created_at: string
  updated_at: string
  completed_at?: string | null
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

/** One side of history-suffix trim (user or assistant message counts). */
export type PersonaHistorySuffixSide = {
  effective: number
  persona_override: number | null
  uses_default: boolean
}

/** GET /api/personas/:id/bulletin `history_suffix` object. */
export type PersonaBulletinHistorySuffix = {
  min_user: PersonaHistorySuffixSide
  min_assistant: PersonaHistorySuffixSide
  defaults: { min_user: number; min_assistant: number }
}

export type PersonaDenseDeliveryInfo = {
  enabled: boolean
  messaging_max_chars: number
  web_max_chars: number
  summary_chars: number
}

export type PersonaAgentEngineInfo = {
  override: string | null
  global: string
  effective: string
  uses_default: boolean
}

/** Must match `OPERATOR_MEMO_MAX_CHARS` on the server. */
export const OPERATOR_MEMO_MAX_CHARS = 4000

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
  persona_id: number
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
  job_kind?: string
  label?: string | null
  tmux_session?: string | null
  shell_command?: string | null
}

export type QueueDiagnosticsResponse = {
  lanes?: QueueLane[]
  background_by_chat?: Record<string, BackgroundJobItem[]>
}

export type ChatSession = {
  id: string
  chat_id: number
  persona_id: number
  title: string
  intent: string
  status: 'active' | 'archived'
  created_at: string
  last_active_at: string
  /** Deprecated: session archive is no longer used. */
  archived_at?: string | null
  /** Deprecated: TTL auto-archive is no longer used. */
  ttl_hours: number
  /** When true, session messages also appear on the main chat timeline. */
  mirror_main_chat?: boolean
}
