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

/** Collect the condition locations used by Edgion and Gateway API resources. */
export function collectResourceConditions(status: unknown): ContextualCondition[] {
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
      const context = `${group.label}: ${reference}`
      conditionArray(record.conditions).forEach((condition) => result.push({ context, condition }))
    })
  }

  return result
}

function conditionColor(status: string): string {
  if (status === 'True') return 'green'
  if (status === 'False') return 'red'
  return 'gold'
}

function ConditionTag({ condition }: { condition: DisplayCondition }) {
  const content = `${condition.type}=${condition.status}`
  if (condition.type === 'ResolvedRefs' && condition.status === 'True') {
    return <Tag data-testid="route-ref-granted" color={conditionColor(condition.status)}>{content}</Tag>
  }
  if (condition.type === 'ResolvedRefs' && condition.reason === 'RefNotPermitted') {
    return <Tag data-testid="route-ref-denied" color={conditionColor(condition.status)}>{content}</Tag>
  }
  return <Tag color={conditionColor(condition.status)}>{content}</Tag>
}

export default function ResourceConditions({
  status,
  compact = false,
  emptyText = 'No status conditions reported',
}: {
  status: unknown
  compact?: boolean
  emptyText?: string
}) {
  const items = collectResourceConditions(status)
  if (items.length === 0) return compact ? <Typography.Text type="secondary">—</Typography.Text> : <Empty description={emptyText} />

  if (compact) {
    return (
      <Space size={[4, 4]} wrap>
        {items.map(({ context, condition }, index) => (
          <Tooltip key={`${context}-${condition.type}-${index}`} title={condition.message || condition.reason}>
            <ConditionTag condition={condition} />
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
            <ConditionTag condition={condition} />
          </Descriptions.Item>
          {condition.reason && <Descriptions.Item label="Reason">{condition.reason}</Descriptions.Item>}
          {condition.message && <Descriptions.Item label="Message">{condition.message}</Descriptions.Item>}
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
