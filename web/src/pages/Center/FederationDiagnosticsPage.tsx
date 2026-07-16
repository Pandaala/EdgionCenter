import { useQuery } from '@tanstack/react-query'
import { Alert, Button, Card, Col, Empty, Row, Space, Statistic, Table, Tag } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import PageHeader from '@/components/PageHeader'
import { centerApi } from '@/api/center'
import { useT } from '@/i18n'

export default function FederationDiagnosticsPage() {
  const t = useT()
  const watch = useQuery({ queryKey: ['center-watch-status'], queryFn: centerApi.watchStatus, staleTime: 15_000 })
  const metadata = useQuery({ queryKey: ['center-metadata-status'], queryFn: centerApi.metadataStoreStatus, staleTime: 15_000 })
  const refresh = () => Promise.all([watch.refetch(), metadata.refetch()])
  const controllers = watch.data?.data ?? []
  const status = metadata.data?.data
  const incomplete = controllers.filter((controller) => !controller.serverId || controller.syncVersion === 0)

  return (
    <div data-testid="federation-diagnostics">
      <PageHeader
        title={t('center.diagnostics.title')}
        subtitle={t('center.diagnostics.subtitle')}
        actions={<Button data-testid="federation-diagnostics-refresh" icon={<ReloadOutlined />} loading={watch.isFetching || metadata.isFetching} onClick={refresh}>{t('btn.refresh')}</Button>}
      />
      {(watch.isError || metadata.isError) && <Alert type="error" showIcon message={t('center.common.loadError')} description={(watch.error as Error | null)?.message ?? (metadata.error as Error | null)?.message} style={{ marginBottom: 16 }} />}
      {incomplete.length > 0 && <Alert type="warning" showIcon message={t('center.diagnostics.incomplete', { n: incomplete.length })} style={{ marginBottom: 16 }} />}
      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col span={8}><Card><Statistic title={t('center.diagnostics.watchControllers')} value={controllers.length} /></Card></Col>
        <Col span={8}><Card><Statistic title={t('center.diagnostics.regionKeys')} value={status?.regionRoutes.length ?? 0} /></Card></Col>
        <Col span={8}><Card><Statistic title={t('center.diagnostics.girKeys')} value={status?.globalConnectionIpRestrictions.length ?? 0} /></Card></Col>
      </Row>
      <Card title={t('center.diagnostics.watchStatus')} style={{ marginBottom: 16 }}>
        <Table
          data-testid="federation-watch-table"
          rowKey="controllerId"
          loading={watch.isLoading}
          dataSource={controllers}
          pagination={false}
          locale={{ emptyText: <Empty description={t('center.diagnostics.noWatch')} /> }}
          columns={[
            { title: t('center.common.controller'), dataIndex: 'controllerId' },
            { title: t('center.diagnostics.syncVersion'), dataIndex: 'syncVersion' },
            { title: t('center.diagnostics.watchServer'), dataIndex: 'serverId', render: (value: string) => value ? <Tag color="green">{value}</Tag> : <Tag color="orange">{t('center.diagnostics.unassigned')}</Tag> },
          ]}
        />
      </Card>
      <Card title={t('center.diagnostics.coverage')}>
        <Space direction="vertical" style={{ width: '100%' }} size={16}>
          <Table
            data-testid="federation-region-metadata-table"
            rowKey="key"
            size="small"
            pagination={false}
            dataSource={status?.regionRoutes ?? []}
            columns={[{ title: 'RegionRoute Key', dataIndex: 'key' }, { title: 'Controllers', dataIndex: 'controllerCount' }]}
          />
          <Table
            data-testid="federation-gir-metadata-table"
            rowKey="key"
            size="small"
            pagination={false}
            dataSource={status?.globalConnectionIpRestrictions ?? []}
            columns={[{ title: 'Global IP Restriction Key', dataIndex: 'key' }, { title: 'Controllers', dataIndex: 'controllerCount' }]}
          />
        </Space>
      </Card>
    </div>
  )
}
