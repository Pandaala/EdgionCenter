import { useEffect, useState, useMemo } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  App, Table, Space, Tag, Typography, Spin, Empty, Button,
  Alert, Tooltip, Select, Popover, AutoComplete, Input,
} from 'antd'
import type { FilterDropdownProps } from 'antd/es/table/interface'
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons'
import {
  regionRouteApi,
  type EffectiveRegionRoute,
  type RegionDef,
  type RegionRouteOverrideRef,
} from '@/api/regionRoute'
import { useCan } from '@/utils/permissions'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'
import { useSearchParams } from 'react-router-dom'

const { Text } = Typography

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** One row is one Controller-local RegionRoute entry. */
export type RegionRouteRow = EffectiveRegionRoute

export function regionRouteRowKey(route: RegionRouteRow): string {
  return `${route.namespace}/${route.pluginName}/${route.entryIndex}`
}

export function writableOverrideRef(route: RegionRouteRow): RegionRouteOverrideRef | null {
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
// FailoverPanel
// ---------------------------------------------------------------------------

function FailoverPanel({
  regions,
  overrideRef,
  onDone,
}: {
  regions: RegionDef[]
  overrideRef: { namespace: string; name: string }
  onDone?: () => void
}) {
  const t = useT()
  const { message } = App.useApp()
  const queryClient = useQueryClient()

  const [pending, setPending] = useState<Record<string, string>>(
    () => Object.fromEntries(regions.map((r) => [r.name, r.failoverTo ?? ''])),
  )
  const effectiveStateKey = regions
    .map((region) => `${region.name}:${region.failoverTo ?? ''}`)
    .join('|')

  // The popover remains mounted while the fleet snapshot refreshes. Keep the
  // controls aligned with the latest effective state instead of preserving the
  // values captured when it first opened.
  useEffect(() => {
    setPending(Object.fromEntries(regions.map((region) => [region.name, region.failoverTo ?? ''])))
  }, [effectiveStateKey, regions])

  const isDirty = regions.some((r) => (r.failoverTo ?? '') !== (pending[r.name] ?? ''))

  const applyMutation = useMutation({
    mutationFn: async () => {
      const changed = regions.filter((r) => (r.failoverTo ?? '') !== (pending[r.name] ?? ''))
      await Promise.all(
        changed.map((region) =>
          regionRouteApi.regionRouteFailover(
            overrideRef.namespace,
            overrideRef.name,
            region.name,
            pending[region.name] ?? '',
          ),
        ),
      )
      await queryClient.refetchQueries({ queryKey: ['region-routes'] })
    },
    onSuccess: () => {
      message.success(t('center.regionRoute.failoverUpdateOk'))
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
            <Tag color={region.failoverTo ? 'orange' : 'green'}>
              {t('center.regionRoute.currentEffective')}: {region.failoverTo ?? t('center.regionRoute.failoverNone')}
            </Tag>
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

function RowActions({ row }: { row: RegionRouteRow }) {
  const t = useT()
  const canWrite = useCan('region-routes:write')
  const [open, setOpen] = useState(false)

  const regions: RegionDef[] = row.regions
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
                  overrideRef={overrideRef}
                  onDone={() => setOpen(false)}
                />
              )}
            </div>
          }
        >
          {failoverButton}
        </Popover>
      )}
    </span>
  )
}

// ---------------------------------------------------------------------------
// ExpandedDetail — per-region table for one Controller
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
  const [searchParams] = useSearchParams()
  const [filter, setFilter] = useState(() => searchParams.get('q') ?? '')

  const { data, isLoading, isError, error, refetch } = useQuery({
    queryKey: ['region-routes'],
    queryFn: () => regionRouteApi.listRegionRoutes(),
    staleTime: 30_000,
  })

  const allItems = useMemo(
    () => (data?.data ?? []) as RegionRouteRow[],
    [data],
  )

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
      render: (_: unknown, r: RegionRouteRow) =>
        r.myRegion ? <Tag color="green">{r.myRegion}</Tag> : <Text type="secondary">—</Text>,
    }

    const regionsCol = {
      title: t('center.regionRoute.regions'),
      key: 'regions',
      render: (_: unknown, r: RegionRouteRow) => <RegionsCell regions={r.regions} />,
    }

    const actionsCol = {
      title: t('center.regionRoute.failoverBtn'),
      key: 'actions',
      render: (_: unknown, r: RegionRouteRow) => <RowActions row={r} />,
    }

    const overrideCol = {
      title: t('center.regionRoute.override'),
      key: 'overrideApplied',
      render: (_: unknown, r: RegionRouteRow) =>
        r.overrideApplied ? (
          <Tag color="orange">{t('center.regionRoute.applied')}</Tag>
        ) : (
          <Text type="secondary">—</Text>
        ),
    }

    return [nameCol, myRegionCol, regionsCol, overrideCol, actionsCol]
  }, [t])

  return (
    <div>
      <PageHeader
        title={t('center.nav.regionRoutes')}
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
            expandedRowRender: (record) => <ControllerExpandedDetail item={record} />,
          }}
        />
      )}
    </div>
  )
}
