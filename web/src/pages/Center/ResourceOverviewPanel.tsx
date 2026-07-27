import { useMemo } from 'react'
import { Card, Col, Empty, Row, Statistic, Typography } from 'antd'
import type { ControllerSummary } from '@/api/center'
import { controllerKindFor } from '@/api/access'
import { listFirstClassResources } from '@/config/resourceCatalog'
import { useT } from '@/i18n'

const RESOURCE_ENTRIES = listFirstClassResources()

/**
 * Renders resource-count cards purely from the `controllers` prop — the
 * summaries already carry `per_kind` from the last StatsReport, so this
 * component makes zero network calls.
 */
export default function ResourceOverviewPanel({ controllers }: { controllers: ControllerSummary[] }) {
  const t = useT()
  const onlineControllers = useMemo(() => controllers.filter((controller) => controller.online), [controllers])
  // Gate on per_kind actually being present, not on stats_state: stats_state
  // is derived from the total alone, so a controller whose per-kind map was
  // dropped by the ingest bound (total present, per_kind null) still reads
  // as 'fresh'. Counting it as "reporting" would contribute 0 to every kind
  // with no partial-data indicator — a silent undercount. Requiring per_kind
  // != null routes that controller into the same partial-data '+' path as a
  // controller that has never reported at all.
  const reportingControllers = useMemo(
    () => onlineControllers.filter((controller) => controller.per_kind != null),
    [onlineControllers],
  )
  const hasMissingStats = reportingControllers.length < onlineControllers.length

  const counts = RESOURCE_ENTRIES.map((entry) => {
    const wireKind = controllerKindFor(entry.kind)
    const count = reportingControllers.reduce(
      (sum, controller) => sum + (controller.per_kind?.[wireKind] ?? 0),
      0,
    )
    return { kind: entry.kind, displayName: entry.displayName, count }
  })
  const total = counts.reduce((sum, item) => sum + item.count, 0)

  return (
    <Card
      data-testid="resource-overview"
      title={t('center.resources.title')}
      style={{ marginBottom: 16 }}
    >
      {onlineControllers.length === 0 ? (
        <Empty description={t('center.resources.noOnlineControllers')} />
      ) : (
        <Row gutter={[12, 12]}>
          <Col xs={24} sm={12} md={8} lg={6}>
            <Card
              data-testid="resource-count-total"
              size="small"
              style={{ height: '100%', background: 'var(--ec-color-bg-subtle)' }}
            >
              <Statistic
                title={t('center.resources.total')}
                value={hasMissingStats ? `${total}+` : total}
                valueStyle={{ color: 'var(--ec-color-brand)' }}
              />
              <Typography.Text type="secondary">
                {t('center.resources.onlineControllers', { n: onlineControllers.length })}
              </Typography.Text>
            </Card>
          </Col>
          {counts.map((item) => (
            <Col key={item.kind} xs={12} sm={12} md={8} lg={6}>
              <Card
                data-testid={`resource-count-${item.kind}`}
                size="small"
                style={{ height: '100%' }}
              >
                <Statistic
                  title={item.displayName}
                  value={hasMissingStats
                    ? (item.count > 0 ? `${item.count}+` : '—')
                    : item.count}
                />
              </Card>
            </Col>
          ))}
        </Row>
      )}
    </Card>
  )
}
