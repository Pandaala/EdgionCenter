import { Descriptions, Empty, Space, Tag, Tooltip, Typography } from 'antd'

export interface DisplayCondition {
  type: string
  status: string
  reason?: string
  message?: string
  observedGeneration?: number
  lastTransitionTime?: string
}

export interface ContextualCondition {
  context: string
  condition: DisplayCondition
}

function conditionArray(value: unknown): DisplayCondition[] {
  if (!Array.isArray(value)) return []
  return value.filter((item): item is DisplayCondition => (
    typeof item === 'object'
      && item !== null
      && typeof (item as DisplayCondition).type === 'string'
      && typeof (item as DisplayCondition).status === 'string'
  ))
}

function referenceLabel(value: unknown, fallback: string): string {
  if (typeof value !== 'object' || value === null) return fallback
  const ref = value as Record<string, unknown>
  const namespace = typeof ref.namespace === 'string' ? `${ref.namespace}/` : ''
  const name = typeof ref.name === 'string' ? ref.name : fallback
  const section = typeof ref.sectionName === 'string' ? `#${ref.sectionName}` : ''
  return `${namespace}${name}${section}`
}

/** Collect a native status payload without following nested envelopes. */
function collectNativeConditions(status: unknown): ContextualCondition[] {
  if (typeof status !== 'object' || status === null) return []
  const source = status as Record<string, unknown>
  const result = conditionArray(source.conditions).map((condition) => ({
    context: 'Resource',
    condition,
  }))

  const contextualGroups: Array<{ key: string; refKey: string; label: string }> = [
    { key: 'parents', refKey: 'parentRef', label: 'Parent' },
    { key: 'ancestors', refKey: 'ancestorRef', label: 'Ancestor' },
    { key: 'listeners', refKey: 'name', label: 'Listener' },
  ]

  for (const group of contextualGroups) {
    const entries: unknown[] = Array.isArray(source[group.key]) ? source[group.key] as unknown[] : []
    entries.forEach((entry, index) => {
      if (typeof entry !== 'object' || entry === null) return
      const record = entry as Record<string, unknown>
      const rawReference = record[group.refKey]
      const reference = typeof rawReference === 'string'
        ? rawReference
        : referenceLabel(rawReference, String(index + 1))
      const owner = typeof record.controllerName === 'string' ? `Controller: ${record.controllerName} / ` : ''
      const context = `${owner}${group.label}: ${reference}`
      conditionArray(record.conditions).forEach((condition) => result.push({ context, condition }))
    })
  }

  return result
}

/** Preserve every deployment observation and its identity in Kubernetes status. */
export function collectResourceConditions(status: unknown): ContextualCondition[] {
  if (typeof status !== 'object' || status === null || Array.isArray(status)) return []
  const source = status as Record<string, unknown>
  if (!('controllers' in source)) return collectNativeConditions(status)
  if (!Array.isArray(source.controllers)) return []

  return source.controllers.flatMap((entry): ContextualCondition[] => {
    if (typeof entry !== 'object' || entry === null) return []
    const observation = entry as Record<string, unknown>
    if (typeof observation.controllerName !== 'string' || !observation.controllerName) return []
    return collectNativeConditions(observation.status).map(({ context, condition }) => ({
      context: context === 'Resource'
        ? `Controller: ${observation.controllerName}`
        : `Controller: ${observation.controllerName} / ${context}`,
      condition,
    }))
  })
}

function conditionColor({ type, status }: DisplayCondition): string {
  if (status !== 'True' && status !== 'False') return 'gold'
  if (type === 'Conflicted') return status === 'True' ? 'red' : 'green'
  if (type === 'PartiallyInvalid') return status === 'True' ? 'orange' : 'green'
  if (['Accepted', 'ResolvedRefs', 'Programmed'].includes(type)) {
    return status === 'True' ? 'green' : 'red'
  }
  // Future condition types may have either polarity. Preserve the value
  // without assigning success or failure until their semantics are known.
  return 'default'
}

function isStale(condition: DisplayCondition, generation?: number): boolean {
  return Number.isSafeInteger(generation) && generation! >= 0
    && Number.isSafeInteger(condition.observedGeneration) && condition.observedGeneration! >= 0
    && condition.observedGeneration! < generation!
}

function ConditionTag({ condition, generation }: { condition: DisplayCondition; generation?: number }) {
  const stale = isStale(condition, generation)
  const content = `${condition.type}=${condition.status}${stale ? ' (stale)' : ''}`
  const color = stale ? 'gold' : conditionColor(condition)
  if (!stale && condition.type === 'ResolvedRefs' && condition.status === 'True') {
    return <Tag data-testid="route-ref-granted" color={color}>{content}</Tag>
  }
  if (!stale && condition.type === 'ResolvedRefs' && condition.reason === 'RefNotPermitted') {
    return <Tag data-testid="route-ref-denied" color={color}>{content}</Tag>
  }
  return <Tag color={color}>{content}</Tag>
}

export default function ResourceConditions({
  status,
  generation,
  compact = false,
  emptyText = 'No status conditions reported',
}: {
  status: unknown
  generation?: number
  compact?: boolean
  emptyText?: string
}) {
  const items = collectResourceConditions(status)
  if (items.length === 0) return compact ? <Typography.Text type="secondary">—</Typography.Text> : <Empty description={emptyText} />

  if (compact) {
    return (
      <Space size={[4, 4]} wrap>
        {items.map(({ context, condition }, index) => (
          <Tooltip key={`${context}-${condition.type}-${index}`} title={[context, isStale(condition, generation) ? `Observed generation ${condition.observedGeneration}; current generation ${generation}` : undefined, condition.reason, condition.message].filter(Boolean).join(' — ')}>
            <ConditionTag condition={condition} generation={generation} />
          </Tooltip>
        ))}
      </Space>
    )
  }

  return (
    <Space direction="vertical" style={{ width: '100%' }}>
      {items.map(({ context, condition }, index) => (
        <Descriptions key={`${context}-${condition.type}-${index}`} bordered size="small" column={1}>
          <Descriptions.Item label="Context">{context}</Descriptions.Item>
          <Descriptions.Item label="Condition">
            <ConditionTag condition={condition} generation={generation} />
          </Descriptions.Item>
          {condition.reason && <Descriptions.Item label="Reason">{condition.reason}</Descriptions.Item>}
          {condition.message && <Descriptions.Item label="Message">{condition.message}</Descriptions.Item>}
          {generation !== undefined && (
            <Descriptions.Item label="Current generation">{generation}</Descriptions.Item>
          )}
          {condition.observedGeneration !== undefined && (
            <Descriptions.Item label="Observed generation">{condition.observedGeneration}</Descriptions.Item>
          )}
          {condition.lastTransitionTime && (
            <Descriptions.Item label="Last transition">{condition.lastTransitionTime}</Descriptions.Item>
          )}
        </Descriptions>
      ))}
    </Space>
  )
}
