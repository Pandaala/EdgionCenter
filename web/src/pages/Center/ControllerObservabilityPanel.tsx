import { useMemo, useState } from 'react'
import { Alert, Badge, Card, Col, Collapse, Row, Segmented, Space, Spin, Statistic, Table, Tag, Tooltip, Typography } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import type { ControllerSummary } from '@/api/center'
import type { ResourceKind } from '@/api/types'
import { useControllerObservability } from '@/hooks/useControllerObservability'
import {
  buildConsistencyRows,
  isCertificateExpiring,
  observations,
  type ConsistencyRow,
  type ControllerResourceSnapshot,
} from '@/utils/controller-observability'

interface CountRow {
  key: string
  group: string
  total: number
  counts: Record<string, number>
}

function countRows(snapshots: ControllerResourceSnapshot[], grouping: 'kind' | 'cluster'): CountRow[] {
  const rows = new Map<string, CountRow>()
  snapshots.forEach((snapshot) => {
    Object.entries(snapshot.resources).forEach(([kind, resources]) => {
      const group = grouping === 'kind' ? kind : snapshot.cluster
      const row = rows.get(group) ?? { key: group, group, total: 0, counts: {} }
      const count = resources?.length ?? 0
      row.total += count
      row.counts[snapshot.controllerId] = (row.counts[snapshot.controllerId] ?? 0) + count
      rows.set(group, row)
    })
  })
  return [...rows.values()].sort((a, b) => b.total - a.total || a.group.localeCompare(b.group))
}

function stateTag(row: ConsistencyRow, controllerId: string) {
  const state = row.controllers[controllerId]
  if (!state?.available) return <Tag>unavailable</Tag>
  if (!state?.present) return <Tag color="red">missing</Tag>
  if (state.issues.length > 0) return <Tooltip title={state.conditionStates.join('\n')}><Tag color="orange">{state.issues.join(', ')}</Tag></Tooltip>
  return <Tooltip title={state.conditionStates.join('\n') || 'No conditions reported'}><Tag color="green">healthy</Tag></Tooltip>
}

export default function ControllerObservabilityPanel({ controllers }: { controllers: ControllerSummary[] }) {
  const onlineControllers = useMemo(() => controllers.filter((controller) => controller.online), [controllers])
  const { snapshots, isLoading, isFetching } = useControllerObservability(onlineControllers)
  const [grouping, setGrouping] = useState<'kind' | 'cluster'>('kind')
  const [consistencyFilter, setConsistencyFilter] = useState<'drift' | 'all'>('drift')
  const allObservations = useMemo(() => snapshots.flatMap(observations), [snapshots])
  const consistencyRows = useMemo(() => buildConsistencyRows(snapshots), [snapshots])
  const visibleConsistency = consistencyFilter === 'drift'
    ? consistencyRows.filter((row) => row.consistent === false)
    : consistencyRows
  const issues = {
    unresolved: allObservations.filter((item) => item.issues.includes('unresolved')).length,
    rejected: allObservations.filter((item) => item.issues.includes('rejected')).length,
    conflicts: allObservations.filter((item) => item.issues.includes('conflict')).length + snapshots.reduce((sum, snapshot) => sum + (snapshot.fileConflicts?.length ?? 0), 0),
    expiring: allObservations.filter((item) => isCertificateExpiring(item.certificateNotAfter)).length,
  }
  const unavailable = snapshots.reduce((total, snapshot) => total + snapshot.errors.length, 0)
  const controllerIds = onlineControllers.map((controller) => controller.controller_id)

  const countColumns: ColumnsType<CountRow> = [
    { title: grouping === 'kind' ? 'Kind' : 'Cluster', dataIndex: 'group', fixed: 'left', width: 210 },
    ...controllerIds.map((controllerId) => ({
      title: controllerId,
      width: 150,
      render: (_: unknown, row: CountRow) => row.counts[controllerId] ?? 0,
    })),
    { title: 'Total', dataIndex: 'total', width: 90 },
  ]
  const consistencyColumns: ColumnsType<ConsistencyRow> = [
    { title: 'Kind', dataIndex: 'kind', width: 180, render: (kind: ResourceKind) => <Tag>{kind}</Tag> },
    { title: 'Namespace', dataIndex: 'namespace', width: 140, render: (value?: string) => value ?? 'cluster' },
    { title: 'Name', dataIndex: 'name', width: 190 },
    { title: 'Result', width: 110, render: (_: unknown, row) => row.consistent === null ? <Tag>unknown</Tag> : row.consistent ? <Tag color="green">consistent</Tag> : <Tag color="red">drift</Tag> },
    ...controllerIds.map((controllerId) => ({
      title: controllerId,
      width: 165,
      render: (_: unknown, row: ConsistencyRow) => stateTag(row, controllerId),
    })),
  ]

  if (onlineControllers.length === 0) return null
  return (
    <Card
      data-testid="controller-observability"
      title={<Space>Fleet resource health {isFetching && <Spin size="small" />}</Space>}
      style={{ marginBottom: 16 }}
    >
      {onlineControllers.length < 2 && (
        <Alert type="info" showIcon message="Connect at least two Controllers to enable cross-Controller consistency comparison." style={{ marginBottom: 12 }} />
      )}
      {unavailable > 0 && (
        <Alert type="warning" showIcon message={`${unavailable} Controller/kind snapshots are unavailable; summaries are partial.`} style={{ marginBottom: 12 }} />
      )}
      {snapshots.some((snapshot) => (snapshot.fileConflicts?.length ?? 0) > 0) && (
        <Alert type="error" showIcon message="Standalone file conflicts detected" description={snapshots.flatMap((snapshot) => (snapshot.fileConflicts ?? []).map((item) => `${snapshot.controllerId}: ${item.kind}/${item.key} (${item.losers.length} losing path(s))`)).join('; ')} style={{ marginBottom: 12 }} />
      )}
      <Row gutter={[12, 12]} style={{ marginBottom: 16 }}>
        <Col xs={12} sm={8} lg={4}><Statistic title="Resources observed" value={allObservations.length} loading={isLoading} /></Col>
        <Col xs={12} sm={8} lg={4}><Statistic title="Config drift" value={consistencyRows.filter((row) => row.consistent === false).length} valueStyle={{ color: '#cf1322' }} /></Col>
        <Col xs={12} sm={8} lg={4}><Statistic title="Unresolved refs" value={issues.unresolved} valueStyle={{ color: '#cf1322' }} /></Col>
        <Col xs={12} sm={8} lg={4}><Statistic title="Rejected" value={issues.rejected} valueStyle={{ color: '#cf1322' }} /></Col>
        <Col xs={12} sm={8} lg={4}><Statistic title="Conflicts" value={issues.conflicts} valueStyle={{ color: '#d46b08' }} /></Col>
        <Col xs={12} sm={8} lg={4}><Statistic title="Certificates ≤30d" value={issues.expiring} valueStyle={{ color: '#d46b08' }} /></Col>
      </Row>

      <Collapse
        defaultActiveKey={['counts', 'consistency']}
        items={[
          {
            key: 'counts',
            label: <Space>Resource inventory <Badge count={allObservations.length} showZero /></Space>,
            children: (
              <>
                <Segmented value={grouping} onChange={(value) => setGrouping(value as 'kind' | 'cluster')} options={[{ label: 'By kind', value: 'kind' }, { label: 'By cluster', value: 'cluster' }]} style={{ marginBottom: 12 }} />
                <Table size="small" rowKey="key" loading={isLoading} dataSource={countRows(snapshots, grouping)} columns={countColumns} pagination={false} scroll={{ x: 'max-content', y: 360 }} />
              </>
            ),
          },
          {
            key: 'consistency',
            label: <Space>Bulk consistency <Badge count={consistencyRows.filter((row) => row.consistent === false).length} showZero /></Space>,
            children: (
              <>
                <Space style={{ marginBottom: 12 }} wrap>
                  <Segmented value={consistencyFilter} onChange={(value) => setConsistencyFilter(value as 'drift' | 'all')} options={[{ label: 'Drift only', value: 'drift' }, { label: 'All resources', value: 'all' }]} />
                  <Typography.Text type="secondary">Spec and operator metadata are compared; status is reported separately.</Typography.Text>
                </Space>
                <Table size="small" rowKey="key" loading={isLoading} dataSource={visibleConsistency} columns={consistencyColumns} pagination={{ pageSize: 20, showSizeChanger: true }} scroll={{ x: 'max-content', y: 480 }} />
              </>
            ),
          },
        ]}
      />
    </Card>
  )
}
