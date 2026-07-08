import { useCallback, useEffect, useState } from 'react'
import { Button, Callout, Flex, Switch, Text, TextArea, TextField } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import type {
  DeterministicPipelineResponse,
  PhaseContextIncludes,
  PipelinePhase,
  PipelinePhaseKind,
  PipelinePolicyConfig,
  PipelineProfile,
  PipelineTransitionCondition,
  PipelineTransitionRule,
} from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
}

const PHASE_KINDS: PipelinePhaseKind[] = [
  'intent_classify',
  'plan_generate',
  'execute_plan',
  'synthesize_delivery',
]

const MODEL_ROUTES = [
  'inherit_global',
  'strategy',
  'local',
] as const

const TRANSITION_CONDITIONS: PipelineTransitionCondition[] = [
  'always',
  'intent_category_conversational',
  'intent_category_question',
  'intent_category_task',
  'intent_needs_clarification',
  'intent_needs_clarification_proceed',
  'plan_empty',
  'execute_any_failed',
  'execute_all_succeeded',
  'channel_web',
  'is_scheduled',
]

const GOTO_OPTIONS = [
  'direct_answer',
  'clarify',
  'finish',
  'phase',
] as const

const POLICY_FIELDS: { key: keyof PipelinePolicyConfig; label: string; hint: string }[] = [
  {
    key: 'heuristic_intent_enabled',
    label: 'Heuristic intent',
    hint: 'Off by default. Fast-path regex shortcuts skip the intent LLM (not recommended).',
  },
  {
    key: 'merged_classify_and_plan_enabled',
    label: 'Merged classify + plan',
    hint: 'Single LLM call for intent + plan on task paths when possible.',
  },
  {
    key: 'skip_consolidate_when_good',
    label: 'Skip full synthesis when output is good',
    hint: 'Polish-only delivery for a single strong step summary.',
  },
  {
    key: 'clarify_on_web_proceed',
    label: 'Clarify on web: proceed on assumptions',
    hint: 'Web channel proceeds instead of asking clarifying questions.',
  },
  {
    key: 'clarify_on_scheduler_proceed',
    label: 'Clarify on scheduler: proceed on assumptions',
    hint: 'Scheduled/background runs proceed on assumptions.',
  },
  {
    key: 'image_input_force_task',
    label: 'Image input forces task',
    hint: 'Treat image attachments as executable tasks.',
  },
  {
    key: 'retry_failed_steps',
    label: 'Retry failed steps',
    hint: 'One automatic retry per plan step after failure.',
  },
  {
    key: 'escalate_to_strategy_on_skill_failure',
    label: 'Escalate skill failures to strategy',
    hint: 'Strategy-tier recovery brief after local step failure.',
  },
  {
    key: 'use_local_for_json_stages',
    label: 'Use local for JSON stages',
    hint: 'Intent/plan JSON calls prefer local tier when routable.',
  },
  {
    key: 'bind_persona_sops_in_plan',
    label: 'Bind persona SOPs in plan',
    hint: 'Off by default. When on, Tier 2 SOPs from persona memory may be injected into the planner. Otherwise only intent candidate_sop_hint.',
  },
]

type BooleanContextKey = {
  [K in keyof PhaseContextIncludes]: PhaseContextIncludes[K] extends boolean ? K : never
}[keyof PhaseContextIncludes]

const CONTEXT_FIELDS: {
  key: BooleanContextKey
  label: string
  hint: string
  kinds?: PipelinePhaseKind[]
}[] = [
  {
    key: 'include_system_prompt',
    label: 'Phase system prompt',
    hint: 'Builtin or custom system prompt for this phase (Layer 3).',
  },
  {
    key: 'include_agent_system_prompt',
    label: 'Agent system prompt',
    hint: 'Full run-prep prompt (AGENTS.md, hooks, skills catalog from classic prep).',
  },
  {
    key: 'include_skills_catalog',
    label: 'Skills catalog',
    hint: 'Allowed skills list in pipeline_cloud_context (intent/plan).',
    kinds: ['intent_classify', 'plan_generate'],
  },
  {
    key: 'include_session_excerpt',
    label: 'Conversation history',
    hint: 'Recent chat excerpt in cloud context, or full messages on direct-answer path.',
  },
  {
    key: 'include_persona_memory',
    label: 'Persona memory',
    hint: 'Tier 1 principles excerpt in cloud context.',
    kinds: ['intent_classify', 'plan_generate'],
  },
  {
    key: 'include_workspace_paths',
    label: 'Workspace paths',
    hint: 'chat_id, persona_id, and tool cwd in cloud context.',
    kinds: ['intent_classify', 'plan_generate'],
  },
  {
    key: 'include_sop_reference',
    label: 'SOP reference',
    hint: 'Inject a vault SOP into the planner when intent names one (or persona SOP bind policy is on). Off by default.',
    kinds: ['plan_generate'],
  },
  {
    key: 'include_current_request',
    label: 'Current request',
    hint: 'Latest user message text in the LLM user payload.',
  },
  {
    key: 'include_prior_step_summaries',
    label: 'Prior step output',
    hint: 'Feed completed step output into the next execute step.',
    kinds: ['execute_plan'],
  },
  {
    key: 'include_step_contract',
    label: 'Step contract',
    hint: 'goal, inputs, skill_name/script/args from the plan step.',
    kinds: ['execute_plan'],
  },
  {
    key: 'include_execution_summary',
    label: 'Execution summary',
    hint: 'Intent goal and step results for consolidate/delivery.',
    kinds: ['synthesize_delivery'],
  },
]

function contextFieldVisible(
  field: (typeof CONTEXT_FIELDS)[number],
  kind: PipelinePhaseKind,
): boolean {
  if (!field.kinds) return true
  return field.kinds.includes(kind)
}

function gotoToString(rule: PipelineTransitionRule): string {
  if (typeof rule.goto === 'string') return rule.goto
  return 'phase'
}

function gotoPhaseId(rule: PipelineTransitionRule): string {
  if (typeof rule.goto === 'object' && rule.goto !== null && 'phase' in rule.goto) {
    return rule.goto.phase
  }
  return ''
}

function makeGoto(kind: string, phaseId: string): PipelineTransitionRule['goto'] {
  if (kind === 'phase') return { phase: phaseId }
  return kind as 'direct_answer' | 'clarify' | 'finish'
}

export function SettingsDeterministicPipelinePanel({ api, onError }: Props) {
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [profile, setProfile] = useState<PipelineProfile | null>(null)
  const [defaults, setDefaults] = useState<PipelineProfile | null>(null)
  const [builtinPrompts, setBuiltinPrompts] = useState<Record<string, string>>({})
  const [agentEngine, setAgentEngine] = useState<string>('classic')
  const [saveNotice, setSaveNotice] = useState<string | null>(null)
  const [expandedPhase, setExpandedPhase] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setSaveNotice(null)
    try {
      const data = await api<DeterministicPipelineResponse>('/api/deterministic-pipeline')
      if (data.profile) setProfile(data.profile)
      if (data.defaults) setDefaults(data.defaults)
      setBuiltinPrompts(data.builtin_prompts ?? {})
      setAgentEngine(data.agent_engine ?? 'classic')
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
      setProfile(null)
    } finally {
      setLoading(false)
    }
  }, [api, onError])

  useEffect(() => {
    void load()
  }, [load])

  function updateProfile(mutator: (p: PipelineProfile) => PipelineProfile) {
    setProfile((prev) => (prev ? mutator(structuredClone(prev)) : prev))
  }

  function updatePhase(index: number, patch: Partial<PipelinePhase>) {
    updateProfile((p) => {
      p.phases[index] = { ...p.phases[index], ...patch }
      return p
    })
  }

  function updateTransition(
    phaseIndex: number,
    ruleIndex: number,
    patch: Partial<PipelineTransitionRule> & { gotoKind?: string; gotoPhase?: string },
  ) {
    updateProfile((p) => {
      const rule = { ...p.phases[phaseIndex].transitions[ruleIndex] }
      if (patch.when != null) rule.when = patch.when
      if (patch.gotoKind != null) {
        rule.goto = makeGoto(patch.gotoKind, patch.gotoPhase ?? gotoPhaseId(rule))
      } else if (patch.gotoPhase != null && gotoToString(rule) === 'phase') {
        rule.goto = { phase: patch.gotoPhase }
      }
      p.phases[phaseIndex].transitions[ruleIndex] = rule
      return p
    })
  }

  function updatePhaseContext(index: number, patch: Partial<PhaseContextIncludes>) {
    updateProfile((p) => {
      p.phases[index].context_includes = {
        ...p.phases[index].context_includes,
        ...patch,
      }
      return p
    })
  }

  function resetPhaseContext(index: number, kind: PipelinePhaseKind) {
    const kindDefaults = defaults?.phases.find((p) => p.kind === kind)?.context_includes
    if (!kindDefaults) return
    updatePhase(index, { context_includes: structuredClone(kindDefaults) })
  }

  async function save(resetDefaults = false) {
    setSaving(true)
    setSaveNotice(null)
    try {
      const res = await api<{ message?: string }>('/api/deterministic-pipeline', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(
          resetDefaults ? { reset_defaults: true } : { profile },
        ),
      })
      setSaveNotice(res.message ?? (resetDefaults ? 'Reset to defaults.' : 'Saved.'))
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  if (loading) {
    return <SettingsPanelSkeleton />
  }

  if (!profile) {
    return <Text size="2" color="gray">Could not load deterministic pipeline profile.</Text>
  }

  const phaseIds = profile.phases.map((p) => p.id)

  return (
    <Flex direction="column" gap="4">
      {agentEngine !== 'deterministic' ? (
        <Callout.Root color="orange" size="1" variant="soft">
          <Callout.Text>
            Agent engine is <strong>{agentEngine}</strong>. This profile applies when Runtime →
            Agent engine is set to <strong>Deterministic</strong>.
          </Callout.Text>
        </Callout.Root>
      ) : null}

      {saveNotice ? (
        <Callout.Root color="green" size="1" variant="soft">
          <Callout.Text>{saveNotice}</Callout.Text>
        </Callout.Root>
      ) : null}

      <Flex gap="2" wrap="wrap">
        <Button size="2" disabled={saving} onClick={() => void save(false)}>
          {saving ? 'Saving…' : 'Save profile'}
        </Button>
        <Button size="2" variant="soft" disabled={saving} onClick={() => void save(true)}>
          Reset to defaults
        </Button>
      </Flex>

      <section className="mc-pipeline-section">
        <Text size="2" weight="bold" className="mb-2 block">
          Phase flow (max 4)
        </Text>
        <Text size="1" color="gray" className="mb-3 block">
          Entry phase: <code>{profile.entry_phase_id}</code> — transitions are evaluated top to
          bottom; first match wins.
        </Text>
        <Flex direction="column" gap="3">
          {profile.phases.map((phase, index) => (
            <div key={phase.id} className="mc-pipeline-phase-card">
              <Flex align="center" justify="between" gap="2" wrap="wrap">
                <Flex align="center" gap="2">
                  <Switch
                    size="1"
                    checked={phase.enabled}
                    onCheckedChange={(checked) => updatePhase(index, { enabled: checked })}
                  />
                  <Text size="2" weight="medium">
                    {phase.label}
                  </Text>
                  <Text size="1" color="gray">
                    ({phase.id})
                  </Text>
                </Flex>
                <Button
                  size="1"
                  variant="ghost"
                  onClick={() =>
                    setExpandedPhase((cur) => (cur === phase.id ? null : phase.id))
                  }
                >
                  {expandedPhase === phase.id ? 'Collapse' : 'Expand'}
                </Button>
              </Flex>
              <Flex gap="2" wrap="wrap" mt="2">
                <label className="mc-pipeline-field">
                  <Text size="1" color="gray">Label</Text>
                  <TextField.Root
                    size="1"
                    value={phase.label}
                    onChange={(e) => updatePhase(index, { label: e.target.value })}
                  />
                </label>
                <label className="mc-pipeline-field">
                  <Text size="1" color="gray">Kind</Text>
                  <select
                    className="mc-pipeline-select"
                    value={phase.kind}
                    onChange={(e) =>
                      updatePhase(index, { kind: e.target.value as PipelinePhaseKind })
                    }
                  >
                    {PHASE_KINDS.map((k) => (
                      <option key={k} value={k}>
                        {k}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="mc-pipeline-field">
                  <Text size="1" color="gray">Model route</Text>
                  <select
                    className="mc-pipeline-select"
                    value={phase.model_route}
                    onChange={(e) =>
                      updatePhase(index, {
                        model_route: e.target.value as PipelinePhase['model_route'],
                      })
                    }
                  >
                    {MODEL_ROUTES.map((r) => (
                      <option key={r} value={r}>
                        {r}
                      </option>
                    ))}
                  </select>
                </label>
              </Flex>

              {expandedPhase === phase.id ? (
                <Flex direction="column" gap="2" mt="3">
                  <Flex align="center" justify="between" gap="2" wrap="wrap">
                    <Text size="1" weight="medium">
                      Context includes
                    </Text>
                    <Button
                      size="1"
                      variant="soft"
                      onClick={() => resetPhaseContext(index, phase.kind)}
                    >
                      Reset context defaults
                    </Button>
                  </Flex>
                  <Text size="1" color="gray" className="mb-1 block">
                    Control what this phase sends to the LLM. Cloud stages (intent/plan) default
                    to rich context; execute defaults to step contract only.
                  </Text>
                  <Flex direction="column" gap="2">
                    {CONTEXT_FIELDS.filter((f) => contextFieldVisible(f, phase.kind)).map(
                      ({ key, label, hint }) => (
                        <Flex key={key} align="center" justify="between" gap="3" wrap="wrap">
                          <Flex direction="column" gap="1" style={{ flex: 1, minWidth: 180 }}>
                            <Text size="1">{label}</Text>
                            <Text size="1" color="gray">
                              {hint}
                            </Text>
                          </Flex>
                          <Switch
                            size="1"
                            checked={phase.context_includes[key]}
                            onCheckedChange={(checked) =>
                              updatePhaseContext(index, { [key]: checked })
                            }
                          />
                        </Flex>
                      ),
                    )}
                  </Flex>

                  {phase.kind === 'execute_plan' ? (
                    <Flex direction="column" gap="2" mt="2">
                      <Text size="1" weight="medium">
                        Prior step handoff
                      </Text>
                      <label className="mc-pipeline-field">
                        <Text size="1" color="gray">Feed mode</Text>
                        <select
                          className="mc-pipeline-select"
                          value={phase.context_includes.prior_step_feed_mode}
                          disabled={!phase.context_includes.include_prior_step_summaries}
                          onChange={(e) =>
                            updatePhaseContext(index, {
                              prior_step_feed_mode: e.target
                                .value as PhaseContextIncludes['prior_step_feed_mode'],
                            })
                          }
                        >
                          <option value="full">Full output (default)</option>
                          <option value="summary">LLM summary</option>
                        </select>
                      </label>
                      {phase.context_includes.prior_step_feed_mode === 'summary' ? (
                        <label className="mc-pipeline-field">
                          <Text size="1" color="gray">
                            Summary prompt (system message; empty = builtin default)
                          </Text>
                          <TextArea
                            size="1"
                            rows={4}
                            value={phase.context_includes.prior_step_summary_prompt}
                            disabled={!phase.context_includes.include_prior_step_summaries}
                            placeholder="Summarize prior step output for the next executor. Keep paths and errors."
                            onChange={(e) =>
                              updatePhaseContext(index, {
                                prior_step_summary_prompt: e.target.value,
                              })
                            }
                          />
                        </label>
                      ) : null}
                      <label className="mc-pipeline-field">
                        <Text size="1" color="gray">Full output max chars (stored + forwarded)</Text>
                        <TextField.Root
                          size="1"
                          type="number"
                          value={String(phase.context_includes.prior_step_full_output_max_chars)}
                          disabled={!phase.context_includes.include_prior_step_summaries}
                          onChange={(e) =>
                            updatePhaseContext(index, {
                              prior_step_full_output_max_chars: Number(e.target.value),
                            })
                          }
                        />
                      </label>
                    </Flex>
                  ) : null}

                  <Text size="1" weight="medium" mt="2">
                    Transitions
                  </Text>
                  {phase.transitions.map((rule, ruleIndex) => (
                    <Flex key={`${phase.id}-${ruleIndex}`} gap="2" wrap="wrap" align="end">
                      <label className="mc-pipeline-field">
                        <Text size="1" color="gray">When</Text>
                        <select
                          className="mc-pipeline-select"
                          value={rule.when}
                          onChange={(e) =>
                            updateTransition(index, ruleIndex, {
                              when: e.target.value as PipelineTransitionCondition,
                            })
                          }
                        >
                          {TRANSITION_CONDITIONS.map((c) => (
                            <option key={c} value={c}>
                              {c}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="mc-pipeline-field">
                        <Text size="1" color="gray">Go to</Text>
                        <select
                          className="mc-pipeline-select"
                          value={gotoToString(rule)}
                          onChange={(e) =>
                            updateTransition(index, ruleIndex, {
                              gotoKind: e.target.value,
                              gotoPhase: phaseIds[0] ?? '',
                            })
                          }
                        >
                          {GOTO_OPTIONS.map((g) => (
                            <option key={g} value={g}>
                              {g}
                            </option>
                          ))}
                        </select>
                      </label>
                      {gotoToString(rule) === 'phase' ? (
                        <label className="mc-pipeline-field">
                          <Text size="1" color="gray">Phase id</Text>
                          <select
                            className="mc-pipeline-select"
                            value={gotoPhaseId(rule)}
                            onChange={(e) =>
                              updateTransition(index, ruleIndex, {
                                gotoPhase: e.target.value,
                              })
                            }
                          >
                            {phaseIds.map((id) => (
                              <option key={id} value={id}>
                                {id}
                              </option>
                            ))}
                          </select>
                        </label>
                      ) : null}
                    </Flex>
                  ))}
                </Flex>
              ) : null}
            </div>
          ))}
        </Flex>
      </section>

      <section className="mc-pipeline-section">
        <Text size="2" weight="bold" className="mb-2 block">
          Layer 1 — Operational knobs
        </Text>
        <Flex gap="2" wrap="wrap">
          {(
            Object.keys(profile.operational) as (keyof PipelineProfile['operational'])[]
          ).map((key) => (
            <label key={key} className="mc-pipeline-field">
              <Text size="1" color="gray">
                {key}
              </Text>
              <TextField.Root
                size="1"
                type="number"
                value={String(profile.operational[key])}
                onChange={(e) =>
                  updateProfile((p) => {
                    p.operational[key] = Number(e.target.value) as never
                    return p
                  })
                }
              />
            </label>
          ))}
        </Flex>
      </section>

      <section className="mc-pipeline-section">
        <Text size="2" weight="bold" className="mb-2 block">
          Layer 2 — Policy toggles
        </Text>
        <Flex direction="column" gap="2">
          {POLICY_FIELDS.map(({ key, label, hint }) => (
            <Flex key={key} align="center" justify="between" gap="3" wrap="wrap">
              <Flex direction="column" gap="1" style={{ flex: 1, minWidth: 200 }}>
                <Text size="2">{label}</Text>
                <Text size="1" color="gray">
                  {hint}
                </Text>
              </Flex>
              <Switch
                size="2"
                checked={profile.policies[key]}
                onCheckedChange={(checked) =>
                  updateProfile((p) => {
                    p.policies[key] = checked
                    return p
                  })
                }
              />
            </Flex>
          ))}
        </Flex>
      </section>

      <section className="mc-pipeline-section">
        <Text size="2" weight="bold" className="mb-2 block">
          Layer 3 — Prompts
        </Text>
        <Text size="1" color="gray" className="mb-3 block">
          Empty system prompt uses the built-in default for each phase kind.
        </Text>
        {profile.phases.map((phase, index) => (
          <div key={`prompt-${phase.id}`} className="mc-pipeline-phase-card mb-3">
            <Flex align="center" justify="between" gap="2" mb="2">
              <Text size="2" weight="medium">
                {phase.label} ({phase.kind})
              </Text>
              <Button
                size="1"
                variant="soft"
                onClick={() =>
                  updatePhase(index, {
                    system_prompt: builtinPrompts[phase.kind] ?? '',
                  })
                }
              >
                Reset to builtin
              </Button>
            </Flex>
            <TextArea
              size="1"
              rows={6}
              value={phase.system_prompt}
              placeholder={
                builtinPrompts[phase.kind]
                  ? '(empty = use builtin default)'
                  : 'Custom system prompt'
              }
              onChange={(e) => updatePhase(index, { system_prompt: e.target.value })}
            />
            <Text size="1" color="gray" mt="1">
              {phase.system_prompt.length === 0
                ? 'Using builtin default'
                : `${phase.system_prompt.length} characters`}
            </Text>
            <label className="mc-pipeline-field mt-2">
              <Text size="1" color="gray">Optional preamble (execute phases)</Text>
              <TextArea
                size="1"
                rows={2}
                value={phase.preamble ?? ''}
                onChange={(e) =>
                  updatePhase(index, { preamble: e.target.value || null })
                }
              />
            </label>
          </div>
        ))}
      </section>

      {defaults ? (
        <Text size="1" color="gray">
          Defaults are available via Reset to defaults. Schema version {profile.version}.
        </Text>
      ) : null}
    </Flex>
  )
}
