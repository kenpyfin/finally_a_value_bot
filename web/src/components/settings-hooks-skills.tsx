import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Checkbox } from '@/components/ui/checkbox'
import { Badge } from '@/components/ui/badge'
import { Select, SelectTrigger, SelectContent, SelectItem, SelectValue } from '@/components/ui/select'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { SettingsPanelSkeleton } from './skeleton'
import type { HookDefinition, PersonaHookSkillPolicy, SkillCatalogEntry } from '../types'

const HOOK_LIFECYCLE_EVENTS = [
  'BeforeTurn',
  'PreToolUse',
  'PostToolUse',
  'PostToolBatch',
  'PreStop',
  'PreDelivery',
  'PostDelivery',
] as const

type HookLifecycleEvent = (typeof HOOK_LIFECYCLE_EVENTS)[number]

const HOOK_EVENT_HINTS: Record<HookLifecycleEvent, string> = {
  BeforeTurn: 'Before the agent loop starts',
  PreToolUse: 'Before each tool call',
  PostToolUse: 'After each tool call',
  PostToolBatch: 'After a batch of tool calls',
  PreStop: 'Before the agent stops',
  PreDelivery: 'After PDQE, before persist/send (dense delivery spill)',
  PostDelivery: 'Before PDQE / focus-sync (despite the name)',
}

function hookUpsertPayload(
  hook: HookDefinition,
  overrides: { event_name?: string } = {},
): Record<string, unknown> {
  return {
    id: hook.id,
    name: hook.name,
    event_name: overrides.event_name ?? hook.event_name,
    matcher: hook.matcher ?? null,
    action_type: hook.action_type,
    action_payload_json: hook.action_payload_json,
    scoped_persona_ids: hook.scoped_persona_ids,
    enabled: hook.enabled,
  }
}

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
  activePersonaId: number | null
}

function setsEqual<T>(a: Set<T>, b: Set<T>): boolean {
  if (a.size !== b.size) return false
  for (const item of a) {
    if (!b.has(item)) return false
  }
  return true
}

function hookAvailableForPersona(hook: HookDefinition): boolean {
  if (hook.scoped_for_persona === false) return false
  if (hook.allowed_for_persona === false) return false
  return true
}

function skillAvailableForPersona(skill: SkillCatalogEntry): boolean {
  return skill.allowed_for_persona !== false
}

function hookScopeLabel(hook: HookDefinition): string {
  if (hook.is_global || hook.scoped_persona_ids == null) return 'Global'
  if (hook.scoped_persona_ids.length === 0) return 'No personas'
  return hook.scoped_persona_ids.map((id) => `#${id}`).join(', ')
}

function hookStatusLabel(
  hook: HookDefinition,
  activePersonaId: number | null,
  restrictHooks: boolean,
  selectedHookIds: Set<number>,
): { text: string; color: 'gray' | 'green' | 'orange' | 'red' } | null {
  if (activePersonaId == null) return null
  if (!hook.enabled) return { text: 'Disabled', color: 'gray' }
  if (hook.scoped_for_persona === false) {
    return restrictHooks && selectedHookIds.has(hook.id)
      ? { text: 'Pending scope on save', color: 'orange' }
      : { text: 'Wrong persona scope', color: 'red' }
  }
  if (restrictHooks && !selectedHookIds.has(hook.id)) {
    return { text: 'Not in allowlist', color: 'orange' }
  }
  if (!restrictHooks && hook.allowed_for_persona === false) {
    return { text: 'Blocked by policy', color: 'red' }
  }
  if (hook.active_for_persona) return { text: 'Active', color: 'green' }
  if (restrictHooks && selectedHookIds.has(hook.id)) {
    return { text: 'Allowed', color: 'green' }
  }
  return { text: 'Available', color: 'green' }
}

const STATUS_BADGE_CLASS: Record<'gray' | 'green' | 'orange' | 'red' | 'blue', string> = {
  gray: 'border-border text-muted-foreground',
  green: 'border-emerald-500/40 text-emerald-600 dark:text-emerald-400',
  orange: 'border-amber-500/40 text-amber-700 dark:text-amber-400',
  red: 'border-destructive/40 text-destructive',
  blue: 'border-sky-500/40 text-sky-700 dark:text-sky-400',
}

function StatusBadge({
  text,
  color,
}: {
  text: string
  color: keyof typeof STATUS_BADGE_CLASS
}) {
  return (
    <Badge variant="outline" className={STATUS_BADGE_CLASS[color]}>
      {text}
    </Badge>
  )
}

function hookPayloadSummary(hook: HookDefinition): string | null {
  const payload = hook.action_payload ?? {}
  const action = hook.action_type.toLowerCase()
  if (action === 'command' && typeof payload.command === 'string') {
    return `command: ${payload.command}`
  }
  if (action === 'prompt' && typeof payload.prompt === 'string') {
    const preview =
      payload.prompt.length > 120 ? `${payload.prompt.slice(0, 120)}…` : payload.prompt
    return `prompt: ${preview}`
  }
  if (action === 'add_context' && typeof payload.additional_context === 'string') {
    const preview =
      payload.additional_context.length > 120
        ? `${payload.additional_context.slice(0, 120)}…`
        : payload.additional_context
    return `context: ${preview}`
  }
  if (action === 'block' && typeof payload.reason === 'string') {
    return `reason: ${payload.reason}`
  }
  if (action.startsWith('builtin_')) {
    return 'Built-in Rust handler'
  }
  return null
}

function formatUpdatedAt(value?: string): string | null {
  if (!value?.trim()) return null
  const parsed = Date.parse(value)
  if (Number.isNaN(parsed)) return value
  return new Date(parsed).toLocaleString()
}

function matchesFilter(text: string, query: string): boolean {
  return text.toLowerCase().includes(query)
}

function hookSearchText(hook: HookDefinition): string {
  const payload = hookPayloadSummary(hook) ?? ''
  return [
    hook.name,
    hook.event_name,
    hook.action_type,
    hook.matcher ?? '',
    payload,
    hookScopeLabel(hook),
  ].join(' ')
}

function skillSearchText(skill: SkillCatalogEntry): string {
  return [
    skill.name,
    skill.description,
    skill.when_to_use ?? '',
    skill.source ?? '',
    (skill.platforms ?? []).join(' '),
    (skill.deps ?? []).join(' '),
  ].join(' ')
}

export function SettingsHooksSkillsPanel({ api, onError, activePersonaId }: Props) {
  const [hooks, setHooks] = useState<HookDefinition[]>([])
  const [skills, setSkills] = useState<SkillCatalogEntry[]>([])
  const [skillsTotal, setSkillsTotal] = useState(0)
  const [skillsRemoteCount, setSkillsRemoteCount] = useState(0)
  const [policy, setPolicy] = useState<PersonaHookSkillPolicy | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)

  const [restrictHooks, setRestrictHooks] = useState(false)
  const [restrictSkills, setRestrictSkills] = useState(false)
  const [selectedHookIds, setSelectedHookIds] = useState<Set<number>>(() => new Set())
  const [selectedSkillNames, setSelectedSkillNames] = useState<Set<string>>(() => new Set())
  const [hookFilter, setHookFilter] = useState('')
  const [skillFilter, setSkillFilter] = useState('')
  const [showAllPersonas, setShowAllPersonas] = useState(false)
  const [hookEventDrafts, setHookEventDrafts] = useState<Record<number, string>>({})
  const [savingHookId, setSavingHookId] = useState<number | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const hooksPath =
        activePersonaId != null
          ? `/api/hooks?persona_id=${activePersonaId}`
          : '/api/hooks'
      const hooksRes = await api<{ hooks?: HookDefinition[] }>(hooksPath)
      const hookList = Array.isArray(hooksRes.hooks) ? hooksRes.hooks : []
      setHooks(hookList)

      const skillsPath =
        activePersonaId != null
          ? `/api/skills?persona_id=${activePersonaId}`
          : '/api/skills'
      const skillsRes = await api<{
        skills?: SkillCatalogEntry[]
        total?: number
        remote_count?: number
      }>(skillsPath)
      const skillList = Array.isArray(skillsRes.skills) ? skillsRes.skills : []
      setSkills(skillList)
      setSkillsTotal(typeof skillsRes.total === 'number' ? skillsRes.total : skillList.length)
      setSkillsRemoteCount(
        typeof skillsRes.remote_count === 'number'
          ? skillsRes.remote_count
          : skillList.filter((s) => s.remote).length,
      )

      if (activePersonaId != null) {
        const policyRes = await api<PersonaHookSkillPolicy>(
          `/api/personas/${activePersonaId}/policy`,
        )
        setPolicy(policyRes)
        const hookRestrict = !policyRes.uses_default_hooks
        const skillRestrict = !policyRes.uses_default_skills
        setRestrictHooks(hookRestrict)
        setRestrictSkills(skillRestrict)
        setSelectedHookIds(
          new Set(
            hookRestrict && Array.isArray(policyRes.allowed_hook_ids)
              ? policyRes.allowed_hook_ids
              : hookList.map((h) => h.id),
          ),
        )
        setSelectedSkillNames(
          new Set(
            skillRestrict && Array.isArray(policyRes.allowed_skill_names)
              ? policyRes.allowed_skill_names
              : skillList.map((s) => s.name),
          ),
        )
      } else {
        setPolicy(null)
        setRestrictHooks(false)
        setRestrictSkills(false)
        setSelectedHookIds(new Set(hookList.map((h) => h.id)))
        setSelectedSkillNames(new Set(skillList.map((s) => s.name)))
      }
      setHookEventDrafts({})
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [activePersonaId, api, onError])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    setShowAllPersonas(false)
    setHookFilter('')
    setSkillFilter('')
    setHookEventDrafts({})
  }, [activePersonaId])

  function hookEventValue(hook: HookDefinition): string {
    return hookEventDrafts[hook.id] ?? hook.event_name
  }

  function hookEventDirty(hook: HookDefinition): boolean {
    return hookEventValue(hook) !== hook.event_name
  }

  function setHookEventDraft(hookId: number, eventName: string) {
    setHookEventDrafts((prev) => ({ ...prev, [hookId]: eventName }))
  }

  function revertHookEventDraft(hookId: number) {
    setHookEventDrafts((prev) => {
      const next = { ...prev }
      delete next[hookId]
      return next
    })
  }

  const personaFilteredHooks = useMemo(() => {
    if (activePersonaId == null || showAllPersonas) return hooks
    return hooks.filter(hookAvailableForPersona)
  }, [activePersonaId, hooks, showAllPersonas])

  const personaFilteredSkills = useMemo(() => {
    if (activePersonaId == null || showAllPersonas) return skills
    return skills.filter(skillAvailableForPersona)
  }, [activePersonaId, showAllPersonas, skills])

  const filteredHooks = useMemo(() => {
    const q = hookFilter.trim().toLowerCase()
    if (!q) return personaFilteredHooks
    return personaFilteredHooks.filter((hook) => matchesFilter(hookSearchText(hook), q))
  }, [hookFilter, personaFilteredHooks])

  const filteredSkills = useMemo(() => {
    const q = skillFilter.trim().toLowerCase()
    if (!q) return personaFilteredSkills
    return personaFilteredSkills.filter((skill) => matchesFilter(skillSearchText(skill), q))
  }, [personaFilteredSkills, skillFilter])

  const hooksAvailableCount = useMemo(
    () => (activePersonaId == null ? hooks.length : hooks.filter(hookAvailableForPersona).length),
    [activePersonaId, hooks],
  )

  const skillsAvailableCount = useMemo(
    () =>
      activePersonaId == null ? skills.length : skills.filter(skillAvailableForPersona).length,
    [activePersonaId, skills],
  )

  const policyDirty = useMemo(() => {
    if (activePersonaId == null || policy == null) return false
    const savedHookRestrict = !policy.uses_default_hooks
    const savedSkillRestrict = !policy.uses_default_skills
    if (restrictHooks !== savedHookRestrict || restrictSkills !== savedSkillRestrict) {
      return true
    }
    const savedHookIds = new Set(
      savedHookRestrict && Array.isArray(policy.allowed_hook_ids)
        ? policy.allowed_hook_ids
        : hooks.map((h) => h.id),
    )
    const savedSkillNames = new Set(
      savedSkillRestrict && Array.isArray(policy.allowed_skill_names)
        ? policy.allowed_skill_names
        : skills.map((s) => s.name),
    )
    return !setsEqual(selectedHookIds, savedHookIds) || !setsEqual(selectedSkillNames, savedSkillNames)
  }, [
    activePersonaId,
    hooks,
    policy,
    restrictHooks,
    restrictSkills,
    selectedHookIds,
    selectedSkillNames,
    skills,
  ])

  function toggleHookId(id: number, checked: boolean) {
    setSelectedHookIds((prev) => {
      const next = new Set(prev)
      if (checked) next.add(id)
      else next.delete(id)
      return next
    })
  }

  function toggleSkillName(name: string, checked: boolean) {
    setSelectedSkillNames((prev) => {
      const next = new Set(prev)
      if (checked) next.add(name)
      else next.delete(name)
      return next
    })
  }

  function setAllHookIds(checked: boolean) {
    setSelectedHookIds(checked ? new Set(hooks.map((h) => h.id)) : new Set())
  }

  function setAllSkillNames(checked: boolean) {
    setSelectedSkillNames(checked ? new Set(skills.map((s) => s.name)) : new Set())
  }

  async function saveHookEvent(hook: HookDefinition) {
    const eventName = hookEventValue(hook)
    if (!HOOK_LIFECYCLE_EVENTS.includes(eventName as HookLifecycleEvent)) {
      onError(`Invalid hook event: ${eventName}`)
      return
    }
    setSavingHookId(hook.id)
    try {
      await api('/api/hooks', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(hookUpsertPayload(hook, { event_name: eventName })),
      })
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setSavingHookId(null)
    }
  }

  async function savePersonaPolicy(useDefaults: boolean) {
    if (activePersonaId == null) return
    setSaving(true)
    try {
      if (!useDefaults && restrictHooks) {
        const hookIdsToEnableScope = hooks
          .filter(
            (hook) =>
              selectedHookIds.has(hook.id) &&
              hook.scoped_for_persona === false &&
              Array.isArray(hook.scoped_persona_ids),
          )
          .map((hook) => hook.id)

        for (const hookId of hookIdsToEnableScope) {
          const hook = hooks.find((h) => h.id === hookId)
          if (!hook || !Array.isArray(hook.scoped_persona_ids)) continue
          const nextScope = Array.from(
            new Set([...hook.scoped_persona_ids, activePersonaId]),
          ).sort((a, b) => a - b)
          await api('/api/hooks', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              ...hookUpsertPayload(hook),
              scoped_persona_ids: nextScope,
            }),
          })
        }
      }

      await api(`/api/personas/${activePersonaId}/policy`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          allowed_hook_ids: useDefaults || !restrictHooks ? null : Array.from(selectedHookIds),
          allowed_skill_names:
            useDefaults || !restrictSkills ? null : Array.from(selectedSkillNames),
        }),
      })
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

  return (
    <div className="flex flex-col gap-3">
      <span className="font-semibold">
        Persona policy {activePersonaId != null ? `(persona #${activePersonaId})` : ''}
      </span>
      {activePersonaId == null ? (
        <span>
          Select a persona to edit per-persona hook/skill availability.
        </span>
      ) : (
        <div className="flex flex-col gap-3">
          <span>
            Default is allow-all. Turn on restriction below, pick hooks/skills with checkboxes in
            the catalogs, then save. Clear all and save to block everything in that category.
            Hook creation and enable/disable are handled by the agent via the{' '}
            <code>create-hook</code> skill.
          </span>

          <div className="flex flex-col gap-1">
            <div className="flex items-center justify-between gap-2 flex-wrap">
              <label>
                <div className="flex items-center gap-2">
                  <Switch
                    size="sm"
                    checked={restrictHooks}
                    disabled={saving}
                    onCheckedChange={(checked) => {
                      setRestrictHooks(checked)
                      if (checked) {
                        setSelectedHookIds(new Set(hooks.map((h) => h.id)))
                      }
                    }}
                  />
                  Restrict hooks to selected
                </div>
              </label>
              {restrictHooks ? (
                <div className="flex gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={saving || hooks.length === 0}
                    onClick={() => setAllHookIds(true)}
                  >
                    Select all hooks
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={saving}
                    onClick={() => setAllHookIds(false)}
                  >
                    Clear hooks
                  </Button>
                </div>
              ) : null}
            </div>
            {restrictHooks ? (
              <span>
                {selectedHookIds.size} of {hooks.length} hooks allowed
                {hooks.length === 0 ? ' — create hooks via agent first' : ''}
              </span>
            ) : (
              <span>All hooks allowed for this persona.</span>
            )}
          </div>

          <div className="flex flex-col gap-1">
            <div className="flex items-center justify-between gap-2 flex-wrap">
              <label>
                <div className="flex items-center gap-2">
                  <Switch
                    size="sm"
                    checked={restrictSkills}
                    disabled={saving}
                    onCheckedChange={(checked) => {
                      setRestrictSkills(checked)
                      if (checked) {
                        setSelectedSkillNames(new Set(skills.map((s) => s.name)))
                      }
                    }}
                  />
                  Restrict skills to selected
                </div>
              </label>
              {restrictSkills ? (
                <div className="flex gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={saving || skills.length === 0}
                    onClick={() => setAllSkillNames(true)}
                  >
                    Select all skills
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={saving}
                    onClick={() => setAllSkillNames(false)}
                  >
                    Clear skills
                  </Button>
                </div>
              ) : null}
            </div>
            {restrictSkills ? (
              <span>
                {selectedSkillNames.size} of {skills.length} skills allowed
              </span>
            ) : (
              <span>All skills allowed for this persona.</span>
            )}
          </div>

          <div className="flex gap-2 flex-wrap">
            <Button
              size="sm"
              disabled={saving || (!policyDirty && !restrictHooks && !restrictSkills)}
              onClick={() => void savePersonaPolicy(false)}
            >
              Save policy
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={saving}
              onClick={() => void savePersonaPolicy(true)}
            >
              Allow all (reset defaults)
            </Button>
          </div>
          <span>
            Saved: hooks {policy?.uses_default_hooks ? 'allow-all' : 'restricted'}; skills{' '}
            {policy?.uses_default_skills ? 'allow-all' : 'restricted'}.
            {policyDirty ? ' Unsaved changes.' : ''}
          </span>
        </div>
      )}

      {activePersonaId != null ? (
        <label>
          <div className="flex items-center gap-2">
            <Checkbox
              size="sm"
              checked={showAllPersonas}
              disabled={saving}
              onCheckedChange={(checked) => setShowAllPersonas(checked === true)}
            />
            Show all personas (include hooks/skills unavailable for this persona)
          </div>
        </label>
      ) : null}

      <span className="font-semibold">Hooks catalog</span>
      <span>
        {activePersonaId != null
          ? showAllPersonas
            ? `Showing all ${hooks.length} hooks (${hooksAvailableCount} available for persona #${activePersonaId})`
            : `Showing ${personaFilteredHooks.length} of ${hooks.length} hooks available for persona #${activePersonaId}`
          : `${hooks.length} defined — select a persona to filter by availability`}
        {activePersonaId != null && restrictHooks
          ? ' · checkboxes when hook restriction is on'
          : ''}
        . Change lifecycle event per hook below; other fields are still managed via the agent.
      </span>
      <Input
        value={hookFilter}
        placeholder="Filter hooks by name, event, action, matcher, or scope"
        onChange={(e) => setHookFilter(e.target.value)}
      />
      {hooks.length === 0 ? (
        <span>No hooks defined yet.</span>
      ) : filteredHooks.length === 0 ? (
        <span>No hooks match the current filter.</span>
      ) : (
        <div className="flex flex-col gap-2 max-h-[420px] overflow-y-auto">
          {filteredHooks.map((hook) => {
            const payloadSummary = hookPayloadSummary(hook)
            const status = hookStatusLabel(
              hook,
              activePersonaId,
              restrictHooks,
              selectedHookIds,
            )
            const updated = formatUpdatedAt(hook.updated_at)
            const eventValue = hookEventValue(hook)
            const eventDirty = hookEventDirty(hook)
            const hookSaving = savingHookId === hook.id
            return (
              <div className="flex items-start gap-2 rounded-md border border-[var(--gray-a6)] p-2" key={hook.id}>
                {activePersonaId != null && restrictHooks ? (
                  <Checkbox
                    size="sm"
                    className="mt-0.5"
                    checked={selectedHookIds.has(hook.id)}
                    disabled={saving}
                    onCheckedChange={(checked) =>
                      toggleHookId(hook.id, checked === true)
                    }
                  />
                ) : null}
                <div className="flex flex-col gap-1 min-w-0 flex-1">
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className="font-medium">
                      #{hook.id} {hook.name}
                    </span>
                    {status ? (
                      <StatusBadge text={status.text} color={status.color} />
                    ) : null}
                    {!hook.enabled ? (
                      <Badge variant="outline">
                        Off
                      </Badge>
                    ) : null}
                    <Badge variant="outline">
                      {hook.is_global ? 'Global scope' : 'Persona scope'}
                    </Badge>
                  </div>
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className="font-medium">
                      Event
                    </span>
                    <Select
                      size="sm"
                      value={eventValue}
                      disabled={saving || hookSaving}
                      onValueChange={(value) => setHookEventDraft(hook.id, value)}
                    >
                      <SelectTrigger className="min-w-[10rem]" ><SelectValue /></SelectTrigger>
                      <SelectContent>
                        {HOOK_LIFECYCLE_EVENTS.map((event) => (
                          <SelectItem key={event} value={event} title={HOOK_EVENT_HINTS[event]}>
                            {event}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    {eventDirty ? (
                      <>
                        <Button
                          size="sm"
                          disabled={saving || hookSaving}
                          onClick={() => void saveHookEvent(hook)}
                        >
                          {hookSaving ? 'Saving…' : 'Save event'}
                        </Button>
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={saving || hookSaving}
                          onClick={() => revertHookEventDraft(hook.id)}
                        >
                          Revert
                        </Button>
                      </>
                    ) : (
                      <span title={HOOK_EVENT_HINTS[eventValue as HookLifecycleEvent]}>
                        {HOOK_EVENT_HINTS[eventValue as HookLifecycleEvent]}
                      </span>
                    )}
                  </div>
                  <div className="flex gap-2 flex-wrap">
                    <Badge variant="secondary">
                      {hook.action_type}
                    </Badge>
                    {hook.matcher ? (
                      <Badge variant="secondary">
                        matcher: {hook.matcher}
                      </Badge>
                    ) : null}
                  </div>
                  <span>
                    Scope: {hookScopeLabel(hook)}
                    {updated ? ` · Updated ${updated}` : ''}
                  </span>
                  {payloadSummary ? (
                    <span className="break-words">
                      {payloadSummary}
                    </span>
                  ) : null}
                </div>
              </div>
            )
          })}
        </div>
      )}

      <span className="font-semibold">Skills catalog</span>
      <span>
        {activePersonaId != null
          ? showAllPersonas
            ? `Showing all ${skillsTotal} skills (${skillsAvailableCount} available for persona #${activePersonaId}, ${skillsRemoteCount} remote)`
            : `Showing ${personaFilteredSkills.length} of ${skillsTotal} skills available for persona #${activePersonaId} (${skillsRemoteCount} remote total)`
          : `${skillsTotal} discovered (${skillsRemoteCount} remote — API or other platform)`}
        {activePersonaId != null && restrictSkills
          ? ' · checkboxes when skill restriction is on'
          : ''}
        . Skills only under <code>shared/workspace/</code> are not listed — move them to{' '}
        <code>skills/</code>.
      </span>
      <Input
        value={skillFilter}
        placeholder="Filter skills by name, description, platforms, or source"
        onChange={(e) => setSkillFilter(e.target.value)}
      />
      {skills.length === 0 ? (
        <span>No skills discovered under workspace/skills.</span>
      ) : filteredSkills.length === 0 ? (
        <span>No skills match the current filter.</span>
      ) : (
        <div className="flex flex-col gap-2 max-h-[420px] overflow-y-auto">
          {filteredSkills.map((skill) => {
            const updated = formatUpdatedAt(skill.updated_at)
            const blocked =
              activePersonaId != null &&
              restrictSkills &&
              !selectedSkillNames.has(skill.name)
            const status =
              activePersonaId == null
                ? null
                : !skillAvailableForPersona(skill)
                  ? { text: 'Blocked by policy', color: 'red' as const }
                  : blocked
                    ? { text: 'Not in allowlist', color: 'orange' as const }
                    : restrictSkills && selectedSkillNames.has(skill.name)
                      ? { text: 'Allowed', color: 'green' as const }
                      : { text: 'Available', color: 'green' as const }
            return (
              <div className="flex items-start gap-2 rounded-md border border-[var(--gray-a6)] p-2" key={skill.name}>
                {activePersonaId != null && restrictSkills ? (
                  <Checkbox
                    size="sm"
                    className="mt-0.5"
                    checked={selectedSkillNames.has(skill.name)}
                    disabled={saving}
                    onCheckedChange={(checked) =>
                      toggleSkillName(skill.name, checked === true)
                    }
                  />
                ) : null}
                <div className="flex flex-col gap-1 min-w-0 flex-1">
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className="font-medium">
                      {skill.name}
                    </span>
                    {status ? (
                      <StatusBadge text={status.text} color={status.color} />
                    ) : null}
                    {skill.remote ? (
                      <Badge variant="outline">
                        Remote
                      </Badge>
                    ) : null}
                    {skill.version ? (
                      <Badge variant="outline">
                        v{skill.version}
                      </Badge>
                    ) : null}
                  </div>
                  <span>{skill.description}</span>
                  {skill.when_to_use ? (
                    <span>
                      When to use: {skill.when_to_use}
                    </span>
                  ) : null}
                  <div className="flex gap-2 flex-wrap">
                    {skill.source ? (
                      <Badge variant="secondary">
                        source: {skill.source}
                      </Badge>
                    ) : null}
                    {(skill.platforms ?? []).map((platform) => (
                      <Badge key={platform} variant="outline" className={STATUS_BADGE_CLASS.blue}>
                        {platform}
                      </Badge>
                    ))}
                    {(skill.deps ?? []).length > 0 ? (
                      <Badge variant="secondary">
                        deps: {(skill.deps ?? []).join(', ')}
                      </Badge>
                    ) : null}
                  </div>
                  {updated ? (
                    <span>
                      Updated {updated}
                    </span>
                  ) : null}
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
