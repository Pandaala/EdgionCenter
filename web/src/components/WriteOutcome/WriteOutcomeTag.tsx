import { Space, Tag, Typography } from 'antd'
import type { OutcomeState, WriteOutcome } from '@/api/writeOutcome'
import { useT } from '@/i18n'

const { Text } = Typography

/**
 * One Ant Design status color per `OutcomeState`, chosen so all six read as
 * visually distinct at a glance: `converged` is the only "success" green,
 * `superseded`/`accepted`/`unknown` are cautionary but not alarming, and
 * `conflict`/`failed` are the two states where nothing was applied.
 */
export const OUTCOME_TAG_COLOR: Record<OutcomeState, string> = {
  converged: 'success',
  superseded: 'warning',
  accepted: 'processing',
  conflict: 'volcano',
  failed: 'error',
  unknown: 'default',
}

function defaultDescribeObserved(observed: unknown): string {
  if (observed === null || observed === undefined) return ''
  if (typeof observed === 'string') return observed
  try {
    const json = JSON.stringify(observed)
    return json.length > 200 ? `${json.slice(0, 200)}…` : json
  } catch {
    return ''
  }
}

/**
 * Human-readable detail line for one `WriteOutcome`, shared between the
 * inline `WriteOutcomeTag` and any toast-style caller reporting a
 * single-controller result. Kept as a plain function so callers that don't
 * want a full React tag can still produce on-brand wording per state.
 */
export function outcomeDetailText(
  t: (key: string, params?: Record<string, string | number>) => string,
  outcome: WriteOutcome,
  describeObserved: (observed: unknown) => string = defaultDescribeObserved,
): string {
  switch (outcome.state) {
    case 'superseded':
      // The prescribed wording, not the raw reason: `reason` is always the
      // fixed "overwritten after the write landed" string, so the value the
      // operator actually needs is the observed document, not that reason.
      return t('writeOutcome.detail.superseded', {
        observed: describeObserved(outcome.observed) || t('writeOutcome.detail.unknown'),
      })
    case 'failed':
    case 'unknown':
      // These two carry the most actionable diagnostic detail from the
      // write core (e.g. "not in the local watch cache"), so prefer it.
      return outcome.reason ?? t(`writeOutcome.detail.${outcome.state}`)
    case 'converged':
    case 'accepted':
    case 'conflict':
      // Prescribed operator-facing wording ("this replica could not
      // confirm" / "refresh and retry") always wins here — the raw CAS/
      // session reason is internal detail, not what the operator needs.
      return t(`writeOutcome.detail.${outcome.state}`)
  }
}

export function WriteOutcomeTag({
  outcome,
  describeObserved,
}: {
  outcome: WriteOutcome
  describeObserved?: (observed: unknown) => string
}) {
  const t = useT()
  const label = t(`writeOutcome.state.${outcome.state}`)
  const detail = outcomeDetailText(t, outcome, describeObserved)
  return (
    <Space direction="vertical" size={0} data-testid={`write-outcome-${outcome.state}`}>
      <Tag color={OUTCOME_TAG_COLOR[outcome.state]}>{label}</Tag>
      {detail && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {detail}
        </Text>
      )}
    </Space>
  )
}

export interface WriteOutcomeItem {
  key: string
  label: string
  outcome: WriteOutcome
  describeObserved?: (observed: unknown) => string
}

/** One labeled row per outcome — used to show a per-controller (or per-region) result list. */
export function WriteOutcomeList({ items }: { items: readonly WriteOutcomeItem[] }) {
  if (items.length === 0) return null
  return (
    <Space direction="vertical" size={8} style={{ width: '100%' }}>
      {items.map((item) => (
        <Space key={item.key} align="start" size={8}>
          <Text strong style={{ minWidth: 140, display: 'inline-block' }}>{item.label}</Text>
          <WriteOutcomeTag outcome={item.outcome} describeObserved={item.describeObserved} />
        </Space>
      ))}
    </Space>
  )
}
