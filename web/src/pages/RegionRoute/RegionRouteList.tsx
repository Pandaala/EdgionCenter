import { useState, useMemo } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Table, Space, Tag, Typography, Spin, Empty, Button,
  Collapse, Alert, Tooltip, Select, Popover, AutoComplete, message, Input,
} from 'antd'
import type { FilterDropdownProps } from 'antd/es/table/interface'
import { ReloadOutlined, WarningOutlined, SearchOutlined } from '@ant-design/icons'
import {
  regionRouteApi,
  type CenterRegionRoute,
  type EffectiveRegionRoute,
  type RegionDef,
  type RegionRouteOverrideRef,
  type ConsistencyResult,
} from '@/api/regionRoute'
import { getActiveControllerId, getAppMode } from '@/utils/proxy'
import { useCan } from '@/utils/permissions'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'
import { useSearchParams } from 'react-router-dom'

const { Text } = Typography

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type RegionRouteRow = CenterRegionRoute | EffectiveRegionRoute

function isCenterRow(r: RegionRouteRow): r is CenterRegionRoute {
  return 'controllers' in r
}

export function regionRouteConsistencyKey(route: RegionRouteRow): string {
  return `${route.namespace}/${route.pluginName}${route.alias ? `/${route.alias}` : ''} (#${route.entryIndex})`
}

export function regionRouteRowKey(route: RegionRouteRow): string {
  return `${route.namespace}/${route.pluginName}/${route.entryIndex}`
}

export function writableOverrideRef(route: RegionRouteRow): RegionRouteOverrideRef | null {
  if (isCenterRow(route)) {
    return Object.values(route.controllers).find((entry) => entry.overrideRef?.permitted)?.overrideRef ?? null
  }
  return route.overrideRef?.permitted ? route.overrideRef : null
}

// ---------------------------------------------------------------------------
// RegionsCell
// ---------------------------------------------------------------------------

function RegionsCell({ regions }: { regions: RegionDef[] }) {
  if (regions.length === 0) return <Text type="secondary">—</Text>
  return (
    <Space direction="vertical" size={4}>
      {regions.map((r) => (
        <Tooltip key={r.name} title={`[${r.hashRange[0]}, ${r.hashRange[1]}]`}>
          <Tag color={r.failoverTo ? 'orange' : 'green'}>
            {r.name}{r.failoverTo ? ` → ${r.failoverTo}` : ''}
          </Tag>
        </Tooltip>
      ))}
    </Space>
  )
}

function RouteConfigSummary({ entry }: { entry: EffectiveRegionRoute }) {
  const t = useT()
  const keyGet = Array.isArray(entry.keyGet) ? entry.keyGet : []
  const routeRules = Array.isArray(entry.routeRules) ? entry.routeRules : []
  return (
    <Space direction="vertical" size={6} style={{ width: '100%' }}>
      <Space wrap>
        <Text type="secondary">{t('center.regionRoute.routeRules')}:</Text>
        {routeRules.length ? routeRules.map((rule, index) => (
          <Tag key={index} color="purple">{rule.type ?? JSON.stringify(rule)}</Tag>
        )) : <Text type="secondary">—</Text>}
        <Text type="secondary">{t('center.regionRoute.hashCalc')}:</Text>
        {entry.hashCalc ? (
          <>
            <Tag>{t('center.regionRoute.algorithm')}: {entry.hashCalc.algorithm ?? '—'}</Tag>
            <Tag>{t('center.regionRoute.modulo')}: {entry.hashCalc.modulo ?? '—'}</Tag>
          </>
        ) : <Text type="secondary">—</Text>}
      </Space>
      <Space wrap>
        <Text type="secondary">Key sources:</Text>
        {keyGet.length ? keyGet.map((source, index) => <Tag key={index}>{JSON.stringify(source)}</Tag>) : <Text type="secondary">—</Text>}
        {entry.overrideRef && <Tag color="orange">Override: {entry.overrideRef.namespace}/{entry.overrideRef.name}</Tag>}
      </Space>
    </Space>
  )
}

// ---------------------------------------------------------------------------
// ConsistencyTag
// ---------------------------------------------------------------------------

function ConsistencyTag({ result }: { result?: ConsistencyResult }) {
  const t = useT()
  if (!result) return <Text type="secondary">—</Text>
  if (result.consistent) return <Tag color="green">{t('center.regionRoute.consistent')}</Tag>

  const content = (
    <div style={{ maxWidth: 400 }}>
      {result.conflicts.map((c, i) => (
        <div key={i} style={{ marginBottom: 6 }}>
          <Text strong>{c}</Text>
        </div>
      ))}
    </div>
  )

  return (
    <Popover title={t('center.regionRoute.consistencyDetail')} content={content} trigger="click">
      <span style={{ fontSize: 18, cursor: 'pointer' }}>⚠️</span>
    </Popover>
  )
}

// ---------------------------------------------------------------------------
// FailoverPanel
// ---------------------------------------------------------------------------

function FailoverPanel({
  regions,
  namespace,
  overrideRef,
  routeIdentity,
  onDone,
}: {
  regions: RegionDef[]
  namespace: string
  overrideRef: { namespace: string; name: string }
  routeIdentity?: { pluginName: string; entryIndex: number }
  onDone?: () => void
}) {
  const t = useT()
  const queryClient = useQueryClient()

  const [pending, setPending] = useState<Record<string, string>>(
    () => Object.fromEntries(regions.map((r) => [r.name, r.failoverTo ?? ''])),
  )

  const isDirty = regions.some((r) => (r.failoverTo ?? '') !== (pending[r.name] ?? ''))

  const applyMutation = useMutation({
    mutationFn: async () => {
      const changed = regions.filter((r) => (r.failoverTo ?? '') !== (pending[r.name] ?? ''))
      await Promise.all(
        changed.map((region) =>
          regionRouteApi.regionRouteFailover(
            routeIdentity ? namespace : overrideRef.namespace,
            routeIdentity ? '' : overrideRef.name,
            region.name,
            pending[region.name] ?? '',
            routeIdentity,
          ),
        ),
      )
      // Allow time for backend gRPC propagation before refreshing
      await new Promise((r) => setTimeout(r, 2000))
    },
    onSuccess: () => {
      message.success(t('center.regionRoute.failoverUpdateOk'))
      queryClient.invalidateQueries({ queryKey: ['region-routes'] })
      queryClient.invalidateQueries({ queryKey: ['region-routes-consistency'] })
      onDone?.()
    },
    onError: (e: unknown) => {
      message.error(t('center.regionRoute.failoverUpdateFail', { err: (e as Error).message }))
    },
  })

  return (
    <div style={{ background: 'var(--ec-color-bg-subtle)', border: '1px solid var(--ec-color-border)', borderRadius: 6, padding: '12px 16px' }}>
      <Space direction="vertical" size={8} style={{ width: '100%' }}>
        {regions.map((region) => (
          <Space key={region.name} size={8} style={{ flexWrap: 'nowrap' }}>
            <Text style={{ width: 110, display: 'inline-block' }} strong>{region.name}</Text>
            <Text type="secondary" style={{ width: 100, display: 'inline-block', fontSize: 12 }}>
              [{region.hashRange[0]}, {region.hashRange[1]}]
            </Text>
            <Select
              data-testid={`region-failover-select-${region.name}`}
              size="small"
              value={pending[region.name] ?? ''}
              disabled={applyMutation.isPending}
              onChange={(v) => setPending((prev) => ({ ...prev, [region.name]: v }))}
              style={{ width: 180 }}
              options={[
                { value: '', label: <Text type="secondary">{t('center.regionRoute.failoverNone')}</Text> },
                ...regions.filter((r) => r.name !== region.name).map((r) => ({ value: r.name, label: r.name })),
              ]}
            />
          </Space>
        ))}
        <Button
          data-testid="region-failover-apply"
          type="primary"
          danger={isDirty}
          disabled={!isDirty}
          loading={applyMutation.isPending}
          onClick={() => applyMutation.mutate()}
          style={{ marginTop: 4 }}
        >
          {t('center.regionRoute.applyToAllN', { n: regions.length })}
        </Button>
      </Space>
    </div>
  )
}

// ---------------------------------------------------------------------------
// RowActions
// ---------------------------------------------------------------------------

function RowActions({
  row,
  consistencyResult,
}: {
  row: RegionRouteRow
  consistencyResult?: ConsistencyResult
}) {
  const t = useT()
  const canWrite = useCan('region-routes:write')
  const [open, setOpen] = useState(false)

  const representative = isCenterRow(row)
    ? Object.entries(row.controllers).sort(([left], [right]) => left.localeCompare(right))[0]?.[1]
    : row
  const regions: RegionDef[] = isCenterRow(row)
    ? (representative?.regions ?? [])
    : row.regions

  // For a Center aggregated row all controllers share the same git-owned base, so the
  // first controller's overrideRef is representative.  For a single-controller row use
  // the field directly.
  const overrideRef = writableOverrideRef(row)

  const failoverDisabled = !overrideRef || !canWrite

  const failoverButton = (
    <Button
      data-testid="region-failover"
      size="small"
      type="primary"
      disabled={failoverDisabled}
      onClick={failoverDisabled ? undefined : () => setOpen(!open)}
    >
      {t('center.regionRoute.failoverBtn')}
    </Button>
  )

  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
      {failoverDisabled ? (
        <Tooltip title="No override configured — create a RegionRouteOverride EdgionConfigData first.">
          {failoverButton}
        </Tooltip>
      ) : (
        <Popover
          open={open}
          onOpenChange={setOpen}
          trigger="click"
          title={t('center.regionRoute.failoverPanel')}
          content={
            <div style={{ minWidth: 380, maxWidth: 500 }}>
              {regions.length === 0 ? (
                <Empty description={t('center.regionRoute.noData')} imageStyle={{ height: 40 }} />
              ) : (
                <FailoverPanel
                  regions={regions}
                  namespace={row.namespace}
                  overrideRef={overrideRef}
                  routeIdentity={isCenterRow(row) ? { pluginName: row.pluginName, entryIndex: row.entryIndex } : undefined}
                  onDone={() => setOpen(false)}
                />
              )}
            </div>
          }
        >
          {failoverButton}
        </Popover>
      )}
      {consistencyResult && !consistencyResult.consistent && (
        <ConsistencyTag result={consistencyResult} />
      )}
    </span>
  )
}

// ---------------------------------------------------------------------------
// ExpandedDetail (center mode — per-controller Collapse)
// ---------------------------------------------------------------------------

function CenterExpandedDetail({
  item,
  consistencyResult,
}: {
  item: CenterRegionRoute
  consistencyResult?: ConsistencyResult
}) {
  const t = useT()
  const controllerEntries = Object.entries(item.controllers)

  if (controllerEntries.length === 0) return <Empty description={t('center.regionRoute.noData')} />

  return (
    <div style={{ padding: '8px 0' }}>
      {consistencyResult && !consistencyResult.consistent && (
        <Alert
          type="warning"
          showIcon
          icon={<WarningOutlined />}
          style={{ marginBottom: 12 }}
          message={t('center.regionRoute.conflictAlert')}
          description={
            <div style={{ marginTop: 4 }}>
              {consistencyResult.conflicts.map((c, i) => (
                <div key={i} style={{ marginBottom: 6 }}>
                  <Text strong>{c}</Text>
                </div>
              ))}
            </div>
          }
        />
      )}
      {controllerEntries.map(([controllerId, entry]) => (
        <Collapse
          key={controllerId}
          size="small"
          style={{ marginBottom: 8 }}
          items={[{
            key: 'main',
            label: (
              <Space>
                <Text strong>{controllerId}</Text>
                {entry.myRegion && (
                  <Tag color="green">{t('center.regionRoute.myRegion')}: {entry.myRegion}</Tag>
                )}
                <Tag color="blue">{entry.regions.length} {t('center.regionRoute.regions')}</Tag>
                {entry.overrideApplied && <Tag color="orange">Override Applied</Tag>}
              </Space>
            ),
            children: (
              <Space direction="vertical" size={12} style={{ width: '100%' }}>
                <RouteConfigSummary entry={entry} />
                <Table
                  size="small"
                  pagination={false}
                  dataSource={entry.regions.map((r, i) => ({ ...r, key: i }))}
                  columns={[
                  {
                    title: t('center.regionRoute.regionName'),
                    dataIndex: 'name',
                    render: (v: string) => <Text strong>{v}</Text>,
                  },
                  {
                    title: t('center.regionRoute.hashRange'),
                    dataIndex: 'hashRange',
                    render: (v: [number, number]) => <Tag color="blue">[{v[0]}, {v[1]}]</Tag>,
                  },
                  {
                    title: t('center.regionRoute.endpoint'),
                    dataIndex: 'backendEndpoint',
                    render: (v: string) => <Text code>{v}</Text>,
                  },
                  {
                    title: t('center.regionRoute.tls'),
                    dataIndex: 'tls',
                    render: (v: boolean) => (
                      <Tag color={v ? 'green' : 'default'}>{v ? 'TLS' : t('center.regionRoute.tlsPlain')}</Tag>
                    ),
                  },
                  {
                    title: t('center.regionRoute.failover'),
                    dataIndex: 'failoverTo',
                    render: (v: string | undefined) =>
                      v ? <Tag color="orange">{v}</Tag> : <Text type="secondary">—</Text>,
                  },
                  ]}
                />
              </Space>
            ),
          }]}
        />
      ))}
    </div>
  )
}

// ---------------------------------------------------------------------------
// ExpandedDetail (controller mode — direct regions table)
// ---------------------------------------------------------------------------

function ControllerExpandedDetail({ item }: { item: EffectiveRegionRoute }) {
  const t = useT()
  return (
    <Space direction="vertical" style={{ width: '100%', padding: '8px 0' }} size={12}>
      <RouteConfigSummary entry={item} />
      <Table
        size="small"
        pagination={false}
        dataSource={item.regions.map((r, i) => ({ ...r, key: i }))}
        columns={[
          {
            title: t('center.regionRoute.regionName'),
            dataIndex: 'name',
            render: (v: string) => <Text strong>{v}</Text>,
          },
          {
            title: t('center.regionRoute.hashRange'),
            dataIndex: 'hashRange',
            render: (v: [number, number]) => <Tag color="blue">[{v[0]}, {v[1]}]</Tag>,
          },
          {
            title: t('center.regionRoute.endpoint'),
            dataIndex: 'backendEndpoint',
            render: (v: string) => <Text code>{v}</Text>,
          },
          {
            title: t('center.regionRoute.tls'),
            dataIndex: 'tls',
            render: (v: boolean) => (
              <Tag color={v ? 'green' : 'default'}>{v ? 'TLS' : t('center.regionRoute.tlsPlain')}</Tag>
            ),
          },
          {
            title: t('center.regionRoute.failover'),
            dataIndex: 'failoverTo',
            render: (v: string | undefined) =>
              v ? <Tag color="orange">{v}</Tag> : <Text type="secondary">—</Text>,
          },
        ]}
      />
    </Space>
  )
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

export default function RegionRouteList() {
  const t = useT()
  // Center aggregated view: app mode is center AND no specific controller is selected.
  const isCenter = getAppMode() === 'center' && !getActiveControllerId()
  const [searchParams] = useSearchParams()
  const [filter, setFilter] = useState(() => searchParams.get('q') ?? '')

  const { data, isLoading, isError, error, refetch } = useQuery({
    queryKey: ['region-routes'],
    queryFn: () => regionRouteApi.listRegionRoutes(),
    staleTime: 30_000,
  })

  const { data: consistencyData } = useQuery({
    queryKey: ['region-routes-consistency'],
    queryFn: () => regionRouteApi.regionRoutesConsistency(),
    staleTime: 30_000,
    enabled: isCenter,
  })

  const allItems = useMemo(
    () => (data?.data ?? []) as RegionRouteRow[],
    [data],
  )

  const consistencyMap = useMemo(() => {
    const map = new Map<string, ConsistencyResult>()
    for (const r of consistencyData?.data ?? []) {
      map.set(`${r.namespace}/${r.name}`, r)
    }
    return map
  }, [consistencyData])

  const filteredItems = useMemo(() => {
    if (!filter) return allItems
    const lf = filter.toLowerCase()
    return allItems.filter((item) =>
      `${item.namespace}/${item.pluginName}`.toLowerCase().includes(lf),
    )
  }, [allItems, filter])

  const filterOptions = useMemo(
    () =>
      [...new Set(allItems.map((i) => `${i.namespace}/${i.pluginName}`))]
        .filter((v) => !filter || v.toLowerCase().includes(filter.toLowerCase()))
        .map((v) => ({ value: v })),
    [allItems, filter],
  )

  const filterIcon = (filtered: boolean) => (
    <SearchOutlined style={{ color: filtered ? 'var(--ec-color-brand)' : undefined }} />
  )

  const searchDropdown = (placeholder: string) => ({
    setSelectedKeys,
    selectedKeys,
    confirm,
    clearFilters,
  }: FilterDropdownProps) => (
    <div style={{ padding: 8 }} onKeyDown={(e) => e.stopPropagation()}>
      <Input
        autoFocus
        placeholder={placeholder}
        value={selectedKeys[0] as string | undefined}
        onChange={(e) => setSelectedKeys(e.target.value ? [e.target.value] : [])}
        onPressEnter={() => confirm()}
        style={{ marginBottom: 8, display: 'block', width: 200 }}
      />
      <Space>
        <Button type="primary" size="small" onClick={() => confirm()}>
          Search
        </Button>
        <Button
          size="small"
          onClick={() => {
            clearFilters?.()
            confirm()
          }}
        >
          Reset
        </Button>
      </Space>
    </div>
  )

  const columns = useMemo(() => {
    const nameCol = {
      title: (
        <>
          {t('center.nav.regionRoutes')}{' '}
          <span style={{ fontSize: 11, color: 'var(--ec-color-text-subtle)', fontWeight: 'normal' }}>
            {t('center.regionRoute.namespacePlugin')}
          </span>
        </>
      ),
      key: 'name',
      sorter: (a: RegionRouteRow, b: RegionRouteRow) =>
        `${a.namespace}/${a.pluginName}`.localeCompare(`${b.namespace}/${b.pluginName}`),
      filterDropdown: searchDropdown(t('center.regionRoute.searchNamespacePlugin')),
      filterIcon,
      onFilter: (value: boolean | bigint | string | number, r: RegionRouteRow) =>
        `${r.namespace}/${r.pluginName}`.toLowerCase().includes(String(value).toLowerCase()),
      render: (_: unknown, r: RegionRouteRow) => (
        <Space direction="vertical" size={2}>
          <Text strong>{r.namespace}/{r.pluginName}</Text>
          {r.alias && (
            <Tag color="blue" style={{ fontSize: 11 }}>
              {r.alias}
            </Tag>
          )}
        </Space>
      ),
    }

    const myRegionCol = {
      title: t('center.regionRoute.myRegion'),
      key: 'myRegion',
      render: (_: unknown, r: RegionRouteRow) => {
        const myRegion = isCenterRow(r)
          ? Object.values(r.controllers)[0]?.myRegion
          : r.myRegion
        return myRegion ? <Tag color="green">{myRegion}</Tag> : <Text type="secondary">—</Text>
      },
    }

    const regionsCol = {
      title: t('center.regionRoute.regions'),
      key: 'regions',
      render: (_: unknown, r: RegionRouteRow) => {
        const regions = isCenterRow(r)
          ? (Object.values(r.controllers)[0]?.regions ?? [])
          : r.regions
        return <RegionsCell regions={regions} />
      },
    }

    const actionsCol = {
      title: t('center.regionRoute.failoverBtn'),
      key: 'actions',
      render: (_: unknown, r: RegionRouteRow) => (
        <RowActions
          row={r}
          consistencyResult={
            isCenter
              ? consistencyMap.get(regionRouteConsistencyKey(r))
              : undefined
          }
        />
      ),
    }

    if (isCenter) {
      return [nameCol, myRegionCol, regionsCol, actionsCol]
    }

    const overrideCol = {
      title: t('center.regionRoute.override'),
      key: 'overrideApplied',
      render: (_: unknown, r: RegionRouteRow) =>
        !isCenterRow(r) && r.overrideApplied ? (
          <Tag color="orange">{t('center.regionRoute.applied')}</Tag>
        ) : (
          <Text type="secondary">—</Text>
        ),
    }

    return [nameCol, myRegionCol, regionsCol, overrideCol, actionsCol]
  }, [t, isCenter, consistencyMap])

  return (
    <div>
      <PageHeader
        title={isCenter ? t('center.regionRoute.regionTitle') : t('center.nav.regionRoutes')}
        subtitle={t('page.subtitle.regionRoutes')}
        actions={
          <Button data-testid="region-refresh" icon={<ReloadOutlined />} onClick={() => refetch()}>
            {t('btn.refresh')}
          </Button>
        }
      />
      <AutoComplete
        data-testid="region-search"
        placeholder={t('center.regionRoute.pluginName')}
        value={filter}
        onChange={setFilter}
        options={filterOptions}
        style={{ width: 300, marginBottom: 16 }}
        allowClear
      />
      {isLoading ? (
        <Spin
          size="large"
          style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', minHeight: 300 }}
        />
      ) : isError ? (
        <Alert
          type="error"
          showIcon
          message={t('center.regionRoute.fetchDetailError')}
          description={error instanceof Error ? error.message : String(error)}
        />
      ) : filteredItems.length === 0 ? (
        <Empty description={t('center.regionRoute.noData')} />
      ) : (
        <Table
          dataSource={filteredItems}
          columns={columns}
          rowKey={regionRouteRowKey}
          pagination={{ pageSize: 10, showTotal: (n) => t('table.totalItems', { n }) }}
          expandable={{
            expandedRowRender: (record) =>
              isCenterRow(record) ? (
                <CenterExpandedDetail
                  item={record}
                  consistencyResult={consistencyMap.get(regionRouteConsistencyKey(record))}
                />
              ) : (
                <ControllerExpandedDetail item={record} />
              ),
          }}
        />
      )}
    </div>
  )
}
