import { useMemo } from 'react'
import { Card, Col, Empty, Row, Space, Spin, Statistic, Typography } from 'antd'
import type { ControllerSummary } from '@/api/center'
import { listFirstClassResources } from '@/config/resourceCatalog'
import { useControllerResourceSnapshots } from '@/hooks/useControllerResourceSnapshots'
import { useT } from '@/i18n'

const RESOURCE_ENTRIES = listFirstClassResources()

export default function ResourceOverviewPanel({ controllers }: { controllers: ControllerSummary[] }) {
  const t = useT()
  const onlineControllers = useMemo(() => controllers.filter((controller) => controller.online), [controllers])
  const { snapshots, isLoading, isFetching } = useControllerResourceSnapshots(onlineControllers)
  const counts = RESOURCE_ENTRIES.map((entry) => ({
    kind: entry.kind,
    displayName: entry.displayName,
    count: snapshots.reduce(
      (total, snapshot) => total + (snapshot.resources[entry.kind]?.length ?? 0),
      0,
    ),
    unavailableControllers: snapshots.filter((snapshot) => snapshot.errors.includes(entry.kind)).length,
  }))
  const total = counts.reduce((sum, item) => sum + item.count, 0)
  const hasUnavailableKinds = counts.some((item) => item.unavailableControllers > 0)

  return (
    <Card
      data-testid="resource-overview"
      title={(
        <Space>
          {t('center.resources.title')}
          {isFetching && <Spin size="small" />}
        </Space>
      )}
      style={{ marginBottom: 16 }}
    >
      {onlineControllers.length === 0 ? (
        <Empty description={t('center.resources.noOnlineControllers')} />
      ) : (
        <>
          <Row gutter={[12, 12]}>
            <Col xs={24} sm={12} md={8} lg={6}>
              <Card
                data-testid="resource-count-total"
                size="small"
                style={{ height: '100%', background: 'var(--ec-color-bg-subtle)' }}
              >
                <Statistic
                  title={t('center.resources.total')}
                  value={hasUnavailableKinds ? `${total}+` : total}
                  loading={isLoading}
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
                    value={item.unavailableControllers > 0
                      ? (item.count > 0 ? `${item.count}+` : '—')
                      : item.count}
                    loading={isLoading}
                  />
                </Card>
              </Col>
            ))}
          </Row>
        </>
      )}
    </Card>
  )
}
