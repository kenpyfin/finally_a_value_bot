import { useCallback, useEffect, useMemo, useState } from 'react'
import { Button, Checkbox, Flex, Select, Switch, Text, TextField } from '@radix-ui/themes'
import type { HookDefinition, PersonaHookSkillPolicy } from '../types'

type SkillRow = {
  name: string
  description: string
  remote?: boolean
  allowed_for_persona?: boolean
}

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
  activePersonaId: number | null
}

const HOOK_EVENTS = [
  'BeforeTurn',
  'PreToolUse',
  'PostToolUse',
  'PostToolBatch',
  'PreStop',
  'PostDelivery',
] as const

const HOOK_ACTION_TYPES = ['block', 'add_context'] as const

const HOOK_PAYLOAD_PRESETS: Record<(typeof HOOK_ACTION_TYPES)[number], { label: string; json: string }[]> = {
  block: [
    { label: 'Policy block', json: '{"reason":"Blocked by policy"}' },
    { label: 'Tool denied', json: '{"reason":"This tool is not allowed for this persona"}' },
  ],
  add_context: [
    {
      label: 'Reminder context',
      json: '{"additional_context":"Remember persona-specific constraints before acting."}',
    },
  ],
}

function setsEqual<T>(a: Set<T>, b: Set<T>): boolean {
  if (a.size !== b.size) return false
  for (const item of a) {
    if (!b.has(item)) return false
  }
  return true
}

export function SettingsHooksSkillsPanel({ api, onError, activePersonaId }: Props) {
  const [hooks, setHooks] = useState<HookDefinition[]>([])
  const [skills, setSkills] = useState<SkillRow[]>([])
  const [skillsTotal, setSkillsTotal] = useState(0)
  const [skillsRemoteCount, setSkillsRemoteCount] = useState(0)
  const [policy, setPolicy] = useState<PersonaHookSkillPolicy | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)

  const [newName, setNewName] = useState('')
  const [newEventName, setNewEventName] = useState<(typeof HOOK_EVENTS)[number]>('PreToolUse')
  const [newMatcher, setNewMatcher] = useState('')
  const [newActionType, setNewActionType] =
    useState<(typeof HOOK_ACTION_TYPES)[number]>('block')
  const [newPayloadJson, setNewPayloadJson] = useState(
    HOOK_PAYLOAD_PRESETS.block[0]?.json ?? '{"reason":"Blocked by policy"}',
  )
  const [newPayloadPreset, setNewPayloadPreset] = useState('0')

  const [restrictHooks, setRestrictHooks] = useState(false)
  const [restrictSkills, setRestrictSkills] = useState(false)
  const [selectedHookIds, setSelectedHookIds] = useState<Set<number>>(() => new Set())
  const [selectedSkillNames, setSelectedSkillNames] = useState<Set<string>>(() => new Set())
  const [skillFilter, setSkillFilter] = useState('')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const hooksRes = await api<{ hooks?: HookDefinition[] }>('/api/hooks')
      const hookList = Array.isArray(hooksRes.hooks) ? hooksRes.hooks : []
      setHooks(hookList)

      const skillsPath =
        activePersonaId != null
          ? `/api/skills?persona_id=${activePersonaId}`
          : '/api/skills'
      const skillsRes = await api<{
        skills?: SkillRow[]
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
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [activePersonaId, api, onError])

  useEffect(() => {
    void load()
  }, [load])

  const payloadPresets = HOOK_PAYLOAD_PRESETS[newActionType]

  useEffect(() => {
    const preset = payloadPresets[Number(newPayloadPreset)] ?? payloadPresets[0]
    if (preset) {
      setNewPayloadJson(preset.json)
    }
  }, [newActionType, newPayloadPreset, payloadPresets])

  const filteredSkills = useMemo(() => {
    const q = skillFilter.trim().toLowerCase()
    if (!q) return skills
    return skills.filter(
      (s) =>
        s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q),
    )
  }, [skillFilter, skills])

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

  async function createHook() {
    if (!newName.trim()) {
      onError('Hook name is required.')
      return
    }
    setSaving(true)
    try {
      await api('/api/hooks', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: newName.trim(),
          event_name: newEventName,
          matcher: newMatcher.trim() || null,
          action_type: newActionType.trim() || 'block',
          action_payload_json: newPayloadJson.trim() || '{}',
          enabled: true,
        }),
      })
      setNewName('')
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  async function toggleHookEnabled(hook: HookDefinition, enabled: boolean) {
    setSaving(true)
    try {
      await api('/api/hooks', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          id: hook.id,
          name: hook.name,
          event_name: hook.event_name,
          matcher: hook.matcher,
          action_type: hook.action_type,
          action_payload_json: hook.action_payload_json,
          enabled,
        }),
      })
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  async function deleteHook(id: number) {
    setSaving(true)
    try {
      await api(`/api/hooks/${id}`, { method: 'DELETE' })
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  async function savePersonaPolicy(useDefaults: boolean) {
    if (activePersonaId == null) return
    setSaving(true)
    try {
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
    return <Text size="2" color="gray">Loading hook and skill policies…</Text>
  }

  return (
    <Flex direction="column" gap="3">
      <Text size="2" weight="bold">Hook definitions</Text>
      <Flex direction="column" gap="2">
        {hooks.length === 0 ? (
          <Text size="1" color="gray">No hooks defined yet.</Text>
        ) : (
          hooks.map((hook) => (
            <Flex key={hook.id} align="center" gap="2" wrap="wrap">
              {activePersonaId != null && restrictHooks ? (
                <Checkbox
                  size="1"
                  checked={selectedHookIds.has(hook.id)}
                  disabled={saving}
                  onCheckedChange={(checked) =>
                    toggleHookId(hook.id, checked === true)
                  }
                />
              ) : null}
              <Text size="1" className="min-w-[180px] font-mono">
                #{hook.id} {hook.name}
              </Text>
              <Text size="1" color="gray">{hook.event_name}</Text>
              <Switch
                size="1"
                checked={hook.enabled}
                disabled={saving}
                onCheckedChange={(checked) => void toggleHookEnabled(hook, checked)}
              />
              <Button
                size="1"
                variant="soft"
                color="red"
                disabled={saving}
                onClick={() => void deleteHook(hook.id)}
              >
                Delete
              </Button>
            </Flex>
          ))
        )}
      </Flex>

      <Text size="2" weight="bold">Create hook</Text>
      <Flex gap="2" wrap="wrap" align="end">
        <TextField.Root
          value={newName}
          placeholder="name"
          onChange={(e) => setNewName(e.target.value)}
        />
        <Select.Root
          value={newEventName}
          onValueChange={(value) => setNewEventName(value as (typeof HOOK_EVENTS)[number])}
        >
          <Select.Trigger className="w-[180px]" placeholder="Lifecycle event" />
          <Select.Content>
            {HOOK_EVENTS.map((evt) => (
              <Select.Item key={evt} value={evt}>
                {evt}
              </Select.Item>
            ))}
          </Select.Content>
        </Select.Root>
        <TextField.Root
          value={newMatcher}
          placeholder="matcher (regex, optional)"
          onChange={(e) => setNewMatcher(e.target.value)}
        />
        <Select.Root
          value={newActionType}
          onValueChange={(value) => {
            setNewActionType(value as (typeof HOOK_ACTION_TYPES)[number])
            setNewPayloadPreset('0')
          }}
        >
          <Select.Trigger className="w-[160px]" placeholder="Action type" />
          <Select.Content>
            {HOOK_ACTION_TYPES.map((actionType) => (
              <Select.Item key={actionType} value={actionType}>
                {actionType}
              </Select.Item>
            ))}
          </Select.Content>
        </Select.Root>
        <Select.Root value={newPayloadPreset} onValueChange={setNewPayloadPreset}>
          <Select.Trigger className="w-[200px]" placeholder="Payload preset" />
          <Select.Content>
            {payloadPresets.map((preset, index) => (
              <Select.Item key={`${newActionType}-${index}`} value={String(index)}>
                {preset.label}
              </Select.Item>
            ))}
          </Select.Content>
        </Select.Root>
        <TextField.Root
          className="min-w-[280px]"
          value={newPayloadJson}
          placeholder="payload JSON (edit if needed)"
          onChange={(e) => setNewPayloadJson(e.target.value)}
        />
        <Button size="1" disabled={saving} onClick={() => void createHook()}>
          Add
        </Button>
      </Flex>

      <Text size="2" weight="bold">
        Persona policy {activePersonaId != null ? `(persona #${activePersonaId})` : ''}
      </Text>
      {activePersonaId == null ? (
        <Text size="1" color="gray">
          Select a persona to edit per-persona hook/skill allowlists.
        </Text>
      ) : (
        <Flex direction="column" gap="3">
          <Text size="1" color="gray">
            Default is allow-all. Turn on restriction below, pick hooks/skills with checkboxes,
            then save. Clear all and save to block everything in that category.
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
                {hooks.length === 0 ? ' — create hooks above first' : ''}
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
              <>
                <TextField.Root
                  value={skillFilter}
                  placeholder="Filter skills by name or description"
                  onChange={(e) => setSkillFilter(e.target.value)}
                />
                <Text size="1" color="gray">
                  {selectedSkillNames.size} of {skills.length} skills allowed
                </Text>
              </>
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

      <Text size="2" weight="bold">Skills catalog</Text>
      <Text size="1" color="gray">
        {skillsTotal} discovered ({skillsRemoteCount} remote — API or other platform
        {activePersonaId != null ? '; checkboxes when skill restriction is on' : ''}).
        Skills only under <code>shared/workspace/</code> are not listed — move them to{' '}
        <code>skills/</code>.
      </Text>
      {skills.length === 0 ? (
        <Text size="1" color="gray">No skills discovered under workspace/skills.</Text>
      ) : (
        <Flex direction="column" gap="1" className="max-h-[360px] overflow-y-auto">
          {filteredSkills.map((s) => {
            const blocked =
              activePersonaId != null &&
              restrictSkills &&
              !selectedSkillNames.has(s.name)
            const allowed =
              activePersonaId == null ||
              !restrictSkills ||
              selectedSkillNames.has(s.name)
            return (
              <Flex key={s.name} align="start" gap="2">
                {activePersonaId != null && restrictSkills ? (
                  <Checkbox
                    size="1"
                    className="mt-0.5"
                    checked={selectedSkillNames.has(s.name)}
                    disabled={saving}
                    onCheckedChange={(checked) =>
                      toggleSkillName(s.name, checked === true)
                    }
                  />
                ) : null}
                <Text size="1" color={s.remote ? 'gray' : undefined}>
                  <strong>{s.name}</strong>
                  {s.remote ? ' (remote skill)' : ''}
                  {blocked ? ' (not in allowlist)' : ''}
                  {!restrictSkills && activePersonaId != null && s.allowed_for_persona === false
                    ? ' (blocked by saved allowlist)'
                    : ''}
                  {allowed && restrictSkills ? ' (allowed)' : ''}
                  {' — '}
                  {s.description}
                </Text>
              </Flex>
            )
          })}
          {filteredSkills.length === 0 ? (
            <Text size="1" color="gray">No skills match the filter.</Text>
          ) : null}
        </Flex>
      )}
    </Flex>
  )
}
