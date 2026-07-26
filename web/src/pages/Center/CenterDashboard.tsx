import { useQuery } from '@tanstack/react-query'
import { Alert, Card, Col, Row, Spin, Statistic } from 'antd'
import { centerApi } from '@/api/center'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useCan } from '@/utils/permissions'
import ResourceOverviewPanel from './ResourceOverviewPanel'

/** The Center landing page summarizes fleet membership and resource counts. */
export default function CenterDashboard() {
  const t = useT()
  const canReadControllers = useCan('controllers:read')
  const controllers = useQuery({
    queryKey: ['center-controllers'],
    queryFn: centerApi.listControllers,
    staleTime: 30_000,
    enabled: canReadControllers,
  })
  const controllerRows = controllers.data?.data ?? []
  const onlineControllers = controllerRows.filter((controller) => controller.online).length
  const clusters = new Set(controllerRows.map((controller) => controller.cluster)).size

  return (
    <div>
      <PageHeader
        title={t('center.dashboard.title')}
        subtitle={t('center.dashboard.subtitle')}
      />
      {canReadControllers && controllers.isError && (
        <Alert
          type="error"
          showIcon
          message={t('center.common.loadError')}
          description={(controllers.error as Error).message}
          style={{ marginBottom: 16 }}
        />
      )}
      {canReadControllers && (
        controllers.isLoading
          ? <Spin size="large" style={{ display: 'block', margin: '48px auto' }} />
          : (
            <>
              <Row gutter={[16, 16]} style={{ marginBottom: 16 }}>
                <Col xs={24} sm={8}>
                  <Card size="small">
                    <Statistic title={t('center.dashboard.controllers')} value={controllerRows.length} />
                  </Card>
                </Col>
                <Col xs={24} sm={8}>
                  <Card size="small">
                    <Statistic
                      title={t('center.dashboard.online')}
                      value={onlineControllers}
                      suffix={`/ ${controllerRows.length}`}
                      valueStyle={controllerRows.length > 0
                        ? { color: onlineControllers === controllerRows.length ? '#389e0d' : '#d46b08' }
                        : undefined}
                    />
                  </Card>
                </Col>
                <Col xs={24} sm={8}>
                  <Card size="small">
                    <Statistic title={t('center.dashboard.clusters')} value={clusters} />
                  </Card>
                </Col>
              </Row>
              <ResourceOverviewPanel controllers={controllerRows} />
            </>
          )
      )}
      {!canReadControllers && (
        <Alert type="info" showIcon message={t('center.dashboard.noAccess')} />
      )}
    </div>
  )
}
