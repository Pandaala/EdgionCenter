import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Alert, AutoComplete, Button, Empty, Space, Spin, Table, Tag, Tooltip, Typography } from 'antd'
import { ReloadOutlined, SettingOutlined } from '@ant-design/icons'
import { useNavigate } from 'react-router-dom'
import PageHeader from '@/components/PageHeader'
import {
  regionRouteApi,
  type CenterRegionRoute,
  type EffectiveRegionRoute,
  type RegionDef,
  type RegionRouteBackendService,
  type RegionRouteServiceUsage,
} from '@/api/regionRoute'
import { useT } from '@/i18n'

const { Text } = Typography

interface ControllerUsage {
  effective: EffectiveRegionRoute | null
  usage: RegionRouteServiceUsage | null
}

export interface ServiceManagementRow {
  key: string
  namespace: string
  pluginName: string
  alias: string | null
  entryIndex: number
  routeKind: string
  routeNamespace: string
  routeName: string
  ruleIndex: number
  controllers: Record<string, ControllerUsage>
  services: RegionRouteBackendService[]
  regions: RegionDef[]
  issues: string[]
  membershipKnown: boolean
}

function usageKey(route: CenterRegionRoute, usage: RegionRouteServiceUsage): string {
  return [
    route.namespace, route.pluginName, route.entryIndex,
    usage.routeKind, usage.routeNamespace, usage.routeName, usage.ruleIndex,
  ].join('/')
}

function backendSignature(services: RegionRouteBackendService[]): string {
  return services
    .map((service) => `${service.namespace}/${service.name}:${service.port ?? ''}`)
    .sort()
    .join('|')
}

function regionSignature(regions: RegionDef[]): string {
  return JSON.stringify([...regions].sort((a, b) => a.name.localeCompare(b.name)))
}

/** Build the service-oriented fleet projection without duplicating one row per Controller. */
export function buildServiceManagementRows(routes: CenterRegionRoute[], onlineControllerIds?: string[]): ServiceManagementRow[] {
  const rows = new Map<string, ServiceManagementRow>()

  for (const route of routes) {
    const routeOnlineControllerIds = route.onlineControllerIds ?? onlineControllerIds
    const online = routeOnlineControllerIds == null ? null : new Set(routeOnlineControllerIds)
    const controllerEntries = Object.entries(route.controllers ?? {}).filter(([controllerId]) => online == null || online.has(controllerId))
    for (const [controllerId, effective] of controllerEntries) {
      for (const usage of effective.serviceUsages ?? []) {
        const key = usageKey(route, usage)
        let row = rows.get(key)
        if (!row) {
          row = {
            key,
            namespace: route.namespace,
            pluginName: route.pluginName,
            alias: route.alias,
            entryIndex: route.entryIndex,
            routeKind: usage.routeKind,
            routeNamespace: usage.routeNamespace,
            routeName: usage.routeName,
            ruleIndex: usage.ruleIndex,
            controllers: {},
            services: usage.backendServices,
            regions: effective.regions ?? [],
            issues: [],
            membershipKnown: routeOnlineControllerIds != null,
          }
          rows.set(key, row)
        }
        row.controllers[controllerId] = { effective, usage }
      }
    }

    for (const row of rows.values()) {
      if (row.namespace !== route.namespace || row.pluginName !== route.pluginName || row.entryIndex !== route.entryIndex) continue
      const expectedControllerIds = routeOnlineControllerIds ?? controllerEntries.map(([controllerId]) => controllerId)
      for (const controllerId of expectedControllerIds) {
        row.controllers[controllerId] ??= { effective: route.controllers[controllerId] ?? null, usage: null }
      }
    }
  }

  for (const row of rows.values()) {
    const entries = Object.entries(row.controllers)
    const missing = entries.filter(([, value]) => value.usage == null).map(([id]) => id)
    const backendVariants = new Set(entries.filter(([, value]) => value.usage).map(([, value]) => backendSignature(value.usage!.backendServices)))
    const regionVariants = new Set(entries.filter(([, value]) => value.effective).map(([, value]) => regionSignature(value.effective!.regions ?? [])))
    row.services = [...new Map(entries.flatMap(([, value]) => value.usage?.backendServices ?? []).map((service) => [
      `${service.namespace}/${service.name}:${service.port ?? ''}`, service,
    ])).values()]
    if (missing.length) row.issues.push(`missingUsage:${missing.join(', ')}`)
    if (backendVariants.size > 1) row.issues.push('backendMismatch')
    if (regionVariants.size > 1) row.issues.push('regionMismatch')
    if (!row.membershipKnown) row.issues.push('membershipUnknown')
  }

  return [...rows.values()].sort((a, b) => a.key.localeCompare(b.key))
}

function RegionsCell({ regions }: { regions: RegionDef[] }) {
  if (!regions.length) return <Text type="secondary">—</Text>
  return <Space wrap>{regions.map((region) => (
    <Tag key={region.name} color={region.failoverTo ? 'orange' : 'green'}>
      {region.name}{region.failoverTo ? ` → ${region.failoverTo}` : ''}
    </Tag>
  ))}</Space>
}

export default function RegionRouteServiceUsagePage() {
  const t = useT()
  const navigate = useNavigate()
  const [filter, setFilter] = useState('')
  const { data, error, isError, isLoading, refetch } = useQuery({
    queryKey: ['region-routes'],
    queryFn: () => regionRouteApi.listRegionRoutes(),
    staleTime: 30_000,
  })
  const rows = useMemo(
    () => buildServiceManagementRows((data?.data ?? []) as CenterRegionRoute[]),
    [data],
  )
  const visible = useMemo(() => {
    const query = filter.trim().toLowerCase()
    if (!query) return rows
    return rows.filter((row) => [
      row.namespace, row.pluginName, row.alias, row.routeKind, row.routeNamespace, row.routeName,
      ...Object.keys(row.controllers),
      ...row.services.flatMap((service) => [service.namespace, service.name]),
    ].filter(Boolean).join('/').toLowerCase().includes(query))
  }, [filter, rows])
  const options = useMemo(() => [...new Set(rows.flatMap((row) => [
    `${row.namespace}/${row.pluginName}`,
    `${row.routeNamespace}/${row.routeName}`,
    ...row.services.map((service) => `${service.namespace}/${service.name}`),
  ]))].map((value) => ({ value })), [rows])

  return (
    <div>
      <PageHeader
        title={t('center.regionService.title')}
        subtitle={t('center.regionService.subtitle')}
        actions={<Button data-testid="region-service-refresh" icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>}
      />
      <Alert type="info" showIcon message={t('center.regionService.sharedFailover')} style={{ marginBottom: 16 }} />
      {isError && <Alert type="error" showIcon message={t('center.common.loadError')} description={(error as Error).message} style={{ marginBottom: 16 }} />}
      <AutoComplete
        data-testid="region-service-search"
        value={filter}
        options={options}
        onChange={setFilter}
        allowClear
        placeholder={t('center.regionService.search')}
        style={{ width: 420, marginBottom: 16 }}
      />
      {isLoading ? <Spin size="large" /> : visible.length === 0 ? (
        <Empty description={t('center.regionService.empty')} />
      ) : (
        <Table
          data-testid="region-service-table"
          rowKey="key"
          dataSource={visible}
          pagination={{ pageSize: 20, showTotal: (n) => t('table.totalItems', { n }) }}
          expandable={{
            expandedRowRender: (row) => (
              <Table
                size="small"
                pagination={false}
                rowKey="controllerId"
                dataSource={Object.entries(row.controllers).map(([controllerId, value]) => ({ controllerId, ...value }))}
                columns={[
                  { title: t('center.common.controller'), dataIndex: 'controllerId', render: (value: string) => <Tag color="blue">{value}</Tag> },
                  { title: t('center.regionRoute.myRegion'), render: (_: unknown, value) => value.effective?.myRegion || '—' },
                  { title: t('center.regionService.backends'), render: (_: unknown, value) => value.usage ? <Space wrap>{value.usage.backendServices.map((service: RegionRouteBackendService) => <Tag key={`${service.namespace}/${service.name}:${service.port ?? ''}`}>{service.namespace}/{service.name}{service.port ? `:${service.port}` : ''}</Tag>)}</Space> : <Tag color="red">{t('center.regionService.missing')}</Tag> },
                  { title: t('center.regionRoute.regions'), render: (_: unknown, value) => <RegionsCell regions={value.effective?.regions ?? []} /> },
                ]}
              />
            ),
          }}
          columns={[
            { title: t('center.regionService.service'), render: (_: unknown, row) => row.services.length ? <Space wrap>{row.services.map((service) => <Tag key={`${service.namespace}/${service.name}:${service.port ?? ''}`} color="green">{service.namespace}/{service.name}{service.port ? `:${service.port}` : ''}</Tag>)}</Space> : <Text type="secondary">—</Text> },
            { title: t('center.regionService.route'), render: (_: unknown, row) => <Space><Tag color="purple">{row.routeKind}</Tag><Text>{row.routeNamespace}/{row.routeName}</Text><Tag>#{row.ruleIndex + 1}</Tag></Space> },
            { title: 'RegionRoute', render: (_: unknown, row) => <Space direction="vertical" size={0}><Text strong>{row.namespace}/{row.pluginName}</Text>{row.alias && <Tag>{row.alias}</Tag>}</Space> },
            { title: t('center.regionRoute.controllers'), render: (_: unknown, row) => {
              const entries = Object.entries(row.controllers)
              const active = entries.filter(([, value]) => value.usage).length
              return <Tooltip title={row.membershipKnown ? entries.map(([id, value]) => `${id}: ${value.usage ? t('center.regionService.present') : t('center.regionService.missing')}`).join('\n') : t('center.regionService.issue.membershipUnknown')}><Tag color={row.membershipKnown && active === entries.length ? 'blue' : 'orange'}>{row.membershipKnown ? `${active}/${entries.length}` : `${active}/?`}</Tag></Tooltip>
            } },
            { title: t('center.regionRoute.regions'), render: (_: unknown, row) => <RegionsCell regions={row.regions} /> },
            { title: t('center.regionService.consistency'), render: (_: unknown, row) => row.issues.length ? <Tooltip title={row.issues.map((issue) => issue.startsWith('missingUsage:') ? t('center.regionService.issueMissing', { controllers: issue.slice('missingUsage:'.length) }) : t(`center.regionService.issue.${issue}`)).join('; ')}><Tag color="orange">{t('center.regionRoute.inconsistent')}</Tag></Tooltip> : <Tag color="green">{t('center.regionRoute.consistent')}</Tag> },
            { title: t('center.regionService.action'), render: (_: unknown, row) => <Button data-testid="region-service-manage-region" icon={<SettingOutlined />} onClick={() => navigate(`/region-routes/region?q=${encodeURIComponent(`${row.namespace}/${row.pluginName}`)}`)}>{t('center.regionService.manageRegion')}</Button> },
          ]}
        />
      )}
    </div>
  )
}
