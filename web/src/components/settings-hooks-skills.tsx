import { useCallback, useEffect, useMemo, useState } from 'react'
import { Badge, Button, Checkbox, Flex, Select, Switch, Text, TextField } from '@radix-ui/themes'
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
    <Flex direction="column" gap="3">
      <Text size="2" weight="bold">
        Persona policy {activePersonaId != null ? `(persona #${activePersonaId})` : ''}
      </Text>
      {activePersonaId == null ? (
        <Text size="1" color="gray">
          Select a persona to edit per-persona hook/skill availability.
        </Text>
      ) : (
        <Flex direction="column" gap="3">
          <Text size="1" color="gray">
            Default is allow-all. Turn on restriction below, pick hooks/skills with checkboxes in
            the catalogs, then save. Clear all and save to block everything in that category.
            Hook creation and enable/disable are handled by the agent via the{' '}
            <code>create-hook</code> skill.
          </Text>

          <Flex direction="column" gap="1">
            <Flex align="center" justify="between" gap="2" wrap="wrap">
              <Text as="label" size="2">
                <Flex align="center" gap="2">
                  <Switch
                    size="1"
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
                </Flex>
              </Text>
              {restrictHooks ? (
                <Flex gap="2">
                  <Button
                    size="1"
                    variant="soft"
                    disabled={saving || hooks.length === 0}
                    onClick={() => setAllHookIds(true)}
                  >
                    Select all hooks
                  </Button>
                  <Button
                    size="1"
                    variant="soft"
                    disabled={saving}
                    onClick={() => setAllHookIds(false)}
                  >
                    Clear hooks
                  </Button>
                </Flex>
              ) : null}
            </Flex>
            {restrictHooks ? (
              <Text size="1" color="gray">
                {selectedHookIds.size} of {hooks.length} hooks allowed
                {hooks.length === 0 ? ' — create hooks via agent first' : ''}
              </Text>
            ) : (
              <Text size="1" color="gray">All hooks allowed for this persona.</Text>
            )}
          </Flex>

          <Flex direction="column" gap="1">
            <Flex align="center" justify="between" gap="2" wrap="wrap">
              <Text as="label" size="2">
                <Flex align="center" gap="2">
                  <Switch
                    size="1"
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
                </Flex>
              </Text>
              {restrictSkills ? (
                <Flex gap="2">
                  <Button
                    size="1"
                    variant="soft"
                    disabled={saving || skills.length === 0}
                    onClick={() => setAllSkillNames(true)}
                  >
                    Select all skills
                  </Button>
                  <Button
                    size="1"
                    variant="soft"
                    disabled={saving}
                    onClick={() => setAllSkillNames(false)}
                  >
                    Clear skills
                  </Button>
                </Flex>
              ) : null}
            </Flex>
            {restrictSkills ? (
              <Text size="1" color="gray">
                {selectedSkillNames.size} of {skills.length} skills allowed
              </Text>
            ) : (
              <Text size="1" color="gray">All skills allowed for this persona.</Text>
            )}
          </Flex>

          <Flex gap="2" wrap="wrap">
            <Button
              size="1"
              disabled={saving || (!policyDirty && !restrictHooks && !restrictSkills)}
              onClick={() => void savePersonaPolicy(false)}
            >
              Save policy
            </Button>
            <Button
              size="1"
              variant="soft"
              disabled={saving}
              onClick={() => void savePersonaPolicy(true)}
            >
              Allow all (reset defaults)
            </Button>
          </Flex>
          <Text size="1" color="gray">
            Saved: hooks {policy?.uses_default_hooks ? 'allow-all' : 'restricted'}; skills{' '}
            {policy?.uses_default_skills ? 'allow-all' : 'restricted'}.
            {policyDirty ? ' Unsaved changes.' : ''}
          </Text>
        </Flex>
      )}

      {activePersonaId != null ? (
        <Text as="label" size="2">
          <Flex align="center" gap="2">
            <Checkbox
              size="1"
              checked={showAllPersonas}
              disabled={saving}
              onCheckedChange={(checked) => setShowAllPersonas(checked === true)}
            />
            Show all personas (include hooks/skills unavailable for this persona)
          </Flex>
        </Text>
      ) : null}

      <Text size="2" weight="bold">Hooks catalog</Text>
      <Text size="1" color="gray">
        {activePersonaId != null
          ? showAllPersonas
            ? `Showing all ${hooks.length} hooks (${hooksAvailableCount} available for persona #${activePersonaId})`
            : `Showing ${personaFilteredHooks.length} of ${hooks.length} hooks available for persona #${activePersonaId}`
          : `${hooks.length} defined — select a persona to filter by availability`}
        {activePersonaId != null && restrictHooks
          ? ' · checkboxes when hook restriction is on'
          : ''}
        . Change lifecycle event per hook below; other fields are still managed via the agent.
      </Text>
      <TextField.Root
        value={hookFilter}
        placeholder="Filter hooks by name, event, action, matcher, or scope"
        onChange={(e) => setHookFilter(e.target.value)}
      />
      {hooks.length === 0 ? (
        <Text size="1" color="gray">No hooks defined yet.</Text>
      ) : filteredHooks.length === 0 ? (
        <Text size="1" color="gray">No hooks match the current filter.</Text>
      ) : (
        <Flex direction="column" gap="2" className="max-h-[420px] overflow-y-auto">
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
              <Flex
                key={hook.id}
                align="start"
                gap="2"
                className="rounded-md border border-[var(--gray-a6)] p-2"
              >
                {activePersonaId != null && restrictHooks ? (
                  <Checkbox
                    size="1"
                    className="mt-0.5"
                    checked={selectedHookIds.has(hook.id)}
                    disabled={saving}
                    onCheckedChange={(checked) =>
                      toggleHookId(hook.id, checked === true)
                    }
                  />
                ) : null}
                <Flex direction="column" gap="1" className="min-w-0 flex-1">
                  <Flex align="center" gap="2" wrap="wrap">
                    <Text size="2" weight="medium">
                      #{hook.id} {hook.name}
                    </Text>
                    {status ? (
                      <Badge size="1" color={status.color} variant="soft">
                        {status.text}
                      </Badge>
                    ) : null}
                    {!hook.enabled ? (
                      <Badge size="1" color="gray" variant="outline">
                        Off
                      </Badge>
                    ) : null}
                    <Badge size="1" variant="outline">
                      {hook.is_global ? 'Global scope' : 'Persona scope'}
                    </Badge>
                  </Flex>
                  <Flex align="center" gap="2" wrap="wrap">
                    <Text size="1" weight="medium">
                      Event
                    </Text>
                    <Select.Root
                      size="1"
                      value={eventValue}
                      disabled={saving || hookSaving}
                      onValueChange={(value) => setHookEventDraft(hook.id, value)}
                    >
                      <Select.Trigger className="min-w-[10rem]" />
                      <Select.Content>
                        {HOOK_LIFECYCLE_EVENTS.map((event) => (
                          <Select.Item key={event} value={event} title={HOOK_EVENT_HINTS[event]}>
                            {event}
                          </Select.Item>
                        ))}
                      </Select.Content>
                    </Select.Root>
                    {eventDirty ? (
                      <>
                        <Button
                          size="1"
                          disabled={saving || hookSaving}
                          onClick={() => void saveHookEvent(hook)}
                        >
                          {hookSaving ? 'Saving…' : 'Save event'}
                        </Button>
                        <Button
                          size="1"
                          variant="soft"
                          disabled={saving || hookSaving}
                          onClick={() => revertHookEventDraft(hook.id)}
                        >
                          Revert
                        </Button>
                      </>
                    ) : (
                      <Text size="1" color="gray" title={HOOK_EVENT_HINTS[eventValue as HookLifecycleEvent]}>
                        {HOOK_EVENT_HINTS[eventValue as HookLifecycleEvent]}
                      </Text>
                    )}
                  </Flex>
                  <Flex gap="2" wrap="wrap">
                    <Badge size="1" variant="soft">
                      {hook.action_type}
                    </Badge>
                    {hook.matcher ? (
                      <Badge size="1" color="orange" variant="soft">
                        matcher: {hook.matcher}
                      </Badge>
                    ) : null}
                  </Flex>
                  <Text size="1" color="gray">
                    Scope: {hookScopeLabel(hook)}
                    {updated ? ` · Updated ${updated}` : ''}
                  </Text>
                  {payloadSummary ? (
                    <Text size="1" className="break-words">
                      {payloadSummary}
                    </Text>
                  ) : null}
                </Flex>
              </Flex>
            )
          })}
        </Flex>
      )}

      <Text size="2" weight="bold">Skills catalog</Text>
      <Text size="1" color="gray">
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
      </Text>
      <TextField.Root
        value={skillFilter}
        placeholder="Filter skills by name, description, platforms, or source"
        onChange={(e) => setSkillFilter(e.target.value)}
      />
      {skills.length === 0 ? (
        <Text size="1" color="gray">No skills discovered under workspace/skills.</Text>
      ) : filteredSkills.length === 0 ? (
        <Text size="1" color="gray">No skills match the current filter.</Text>
      ) : (
        <Flex direction="column" gap="2" className="max-h-[420px] overflow-y-auto">
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
              <Flex
                key={skill.name}
                align="start"
                gap="2"
                className="rounded-md border border-[var(--gray-a6)] p-2"
              >
                {activePersonaId != null && restrictSkills ? (
                  <Checkbox
                    size="1"
                    className="mt-0.5"
                    checked={selectedSkillNames.has(skill.name)}
                    disabled={saving}
                    onCheckedChange={(checked) =>
                      toggleSkillName(skill.name, checked === true)
                    }
                  />
                ) : null}
                <Flex direction="column" gap="1" className="min-w-0 flex-1">
                  <Flex align="center" gap="2" wrap="wrap">
                    <Text size="2" weight="medium">
                      {skill.name}
                    </Text>
                    {status ? (
                      <Badge size="1" color={status.color} variant="soft">
                        {status.text}
                      </Badge>
                    ) : null}
                    {skill.remote ? (
                      <Badge size="1" color="gray" variant="outline">
                        Remote
                      </Badge>
                    ) : null}
                    {skill.version ? (
                      <Badge size="1" variant="outline">
                        v{skill.version}
                      </Badge>
                    ) : null}
                  </Flex>
                  <Text size="1">{skill.description}</Text>
                  {skill.when_to_use ? (
                    <Text size="1" color="gray">
                      When to use: {skill.when_to_use}
                    </Text>
                  ) : null}
                  <Flex gap="2" wrap="wrap">
                    {skill.source ? (
                      <Badge size="1" variant="soft">
                        source: {skill.source}
                      </Badge>
                    ) : null}
                    {(skill.platforms ?? []).map((platform) => (
                      <Badge key={platform} size="1" color="blue" variant="soft">
                        {platform}
                      </Badge>
                    ))}
                    {(skill.deps ?? []).length > 0 ? (
                      <Badge size="1" color="orange" variant="soft">
                        deps: {(skill.deps ?? []).join(', ')}
                      </Badge>
                    ) : null}
                  </Flex>
                  {updated ? (
                    <Text size="1" color="gray">
                      Updated {updated}
                    </Text>
                  ) : null}
                </Flex>
              </Flex>
            )
          })}
        </Flex>
      )}
    </Flex>
  )
}
