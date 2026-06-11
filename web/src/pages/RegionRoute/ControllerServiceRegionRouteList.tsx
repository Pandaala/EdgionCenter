import { useState, useMemo } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Table, Space, Tag, Typography, Spin, Empty, Button,
  Select, Popover, AutoComplete, message, Input,
} from 'antd'
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons'
import {
  regionRouteApi,
  type ServiceRegionRouteEntry,
} from '@/api/regionRoute'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'

const { Text } = Typography

// ---------------------------------------------------------------------------
// FailoverPanel
// ---------------------------------------------------------------------------

function FailoverPanel({
  regions,
  namespace,
  name,
  onDone,
}: {
  regions: Array<{ name: string; failoverTo?: string }>
  namespace: string
  name: string
  onDone?: () => void
}) {
  const t = useT()
  const queryClient = useQueryClient()

  const [pending, setPending] = useState<Record<string, string>>(
    () => Object.fromEntries(regions.map((r) => [r.name, r.failoverTo ?? ''])),
  )

  const isDirty = regions.some((r) => (r.failoverTo ?? '') !== (pending[r.name] ?? ''))
  const allRegionNames = regions.map((r) => r.name)

  const applyMutation = useMutation({
    mutationFn: async () => {
      const changed = regions.filter((r) => (r.failoverTo ?? '') !== (pending[r.name] ?? ''))
      await Promise.all(
        changed.map((region) =>
          regionRouteApi.serviceRegionRouteFailover(namespace, name, region.name, pending[region.name] ?? ''),
        ),
      )
      await new Promise((r) => setTimeout(r, 2000))
    },
    onSuccess: () => {
      message.success(t('center.regionRoute.failoverUpdateOk'))
      queryClient.invalidateQueries({ queryKey: ['controller-service-region-routes'] })
      onDone?.()
    },
    onError: (e: any) => {
      message.error(t('center.regionRoute.failoverUpdateFail', { err: e.message }))
    },
  })

  return (
    <div style={{ background: 'var(--ec-color-bg-subtle)', border: '1px solid var(--ec-color-border)', borderRadius: 6, padding: '12px 16px' }}>
      <Space direction="vertical" size={8} style={{ width: '100%' }}>
        {regions.map((region) => (
          <Space key={region.name} size={8} style={{ flexWrap: 'nowrap' }}>
            <Text style={{ width: 110, display: 'inline-block' }} strong>{region.name}</Text>
            <Select
              size="small"
              value={pending[region.name] ?? ''}
              disabled={applyMutation.isPending}
              onChange={(v) => setPending((prev) => ({ ...prev, [region.name]: v }))}
              style={{ width: 180 }}
              options={[
                { value: '', label: <Text type="secondary">{t('center.regionRoute.failoverNone')}</Text> },
                ...allRegionNames.filter((n) => n !== region.name).map((n) => ({ value: n, label: n })),
              ]}
            />
          </Space>
        ))}
        <Button
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

function RowActions({ entry }: { entry: ServiceRegionRouteEntry }) {
  const t = useT()
  const [open, setOpen] = useState(false)

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger="click"
      title={t('center.regionRoute.failoverPanel')}
      content={
        <div style={{ minWidth: 380, maxWidth: 500 }}>
          {entry.regions.length === 0 ? (
            <Empty description={t('center.regionRoute.noData')} imageStyle={{ height: 40 }} />
          ) : (
            <FailoverPanel
              regions={entry.regions}
              namespace={(entry.pmNamespace ?? entry.namespace ?? '')}
              name={(entry.pmName ?? entry.name ?? '')}
              onDone={() => setOpen(false)}
            />
          )}
        </div>
      }
    >
      <Button size="small" type="primary">
        {t('center.regionRoute.failoverBtn')}
      </Button>
    </Popover>
  )
}

// ---------------------------------------------------------------------------
// ExpandedDetail
// ---------------------------------------------------------------------------

function ExpandedDetail({ entry }: { entry: ServiceRegionRouteEntry }) {
  const t = useT()
  return (
    <Space direction="vertical" style={{ width: '100%', padding: '8px 0' }} size={12}>
      <Table
        size="small"
        pagination={false}
        dataSource={entry.regions.map((r, i) => ({ ...r, key: i }))}
        columns={[
          { title: t('center.regionRoute.regionName'), dataIndex: 'name', render: (v: string) => <Text strong>{v}</Text> },
          { title: t('center.regionRoute.failover'), dataIndex: 'failoverTo', render: (v: string | undefined) => v ? <Tag color="orange">{v}</Tag> : <Text type="secondary">—</Text> },
        ]}
      />
      {entry.refPlugins.length > 0 && (
        <div>
          <Text type="secondary" style={{ fontSize: 12 }}>Ref Plugins</Text>
          <div style={{ marginTop: 4 }}>
            <Space wrap>{entry.refPlugins.map((p, i) => {
              // Controller-side wire returns objects ({kind, namespace, name}); Center wire returns strings.
              const label = typeof p === 'string' ? p : `${p.namespace ?? ''}/${p.name ?? ''}`
              return <Tag key={i} color="purple">{label}</Tag>
            })}</Space>
          </div>
        </div>
      )}
    </Space>
  )
}

// ---------------------------------------------------------------------------
// Main page
// ---------------------------------------------------------------------------

export default function ControllerServiceRegionRouteList() {
  const t = useT()
  const [filter, setFilter] = useState('')

  const { data, isLoading, refetch } = useQuery({
    queryKey: ['controller-service-region-routes'],
    queryFn: () => regionRouteApi.listServiceRegionRoutes(),
    staleTime: 30_000,
  })

  const allItems = useMemo(() => (data?.data ?? []) as ServiceRegionRouteEntry[], [data])

  const filteredItems = useMemo(() => {
    if (!filter) return allItems
    const lf = filter.toLowerCase()
    return allItems.filter((item) => `${(item.pmNamespace ?? item.namespace ?? '')}/${(item.pmName ?? item.name ?? '')}`.toLowerCase().includes(lf))
  }, [allItems, filter])

  const filterOptions = useMemo(
    () => [...new Set(allItems.map((i) => `${(i.pmNamespace ?? i.namespace ?? '')}/${(i.pmName ?? i.name ?? '')}`))]
      .filter((v) => !filter || v.toLowerCase().includes(filter.toLowerCase()))
      .map((v) => ({ value: v })),
    [allItems, filter],
  )

  const filterIcon = (filtered: boolean) => (
    <SearchOutlined style={{ color: filtered ? 'var(--ec-color-brand)' : undefined }} />
  )
  const searchDropdown = (placeholder: string) => ({ setSelectedKeys, selectedKeys, confirm, clearFilters }: {
    setSelectedKeys: (keys: React.Key[]) => void
    selectedKeys: React.Key[]
    confirm: () => void
    clearFilters?: () => void
  }) => (
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
        <Button type="primary" size="small" onClick={() => confirm()}>Search</Button>
        <Button size="small" onClick={() => { clearFilters?.(); confirm() }}>Reset</Button>
      </Space>
    </div>
  )

  const columns = useMemo(() => [
    {
      title: t('center.regionRoute.pmName'),
      key: 'name',
      sorter: (a: ServiceRegionRouteEntry, b: ServiceRegionRouteEntry) => {
        const an = `${a.pmNamespace ?? a.namespace ?? ''}/${a.pmName ?? a.name ?? ''}`
        const bn = `${b.pmNamespace ?? b.namespace ?? ''}/${b.pmName ?? b.name ?? ''}`
        return an.localeCompare(bn)
      },
      filterDropdown: searchDropdown('Search namespace/name'),
      filterIcon,
      onFilter: (value: React.Key | boolean, r: ServiceRegionRouteEntry) => {
        const full = `${r.pmNamespace ?? r.namespace ?? ''}/${r.pmName ?? r.name ?? ''}`
        return full.toLowerCase().includes(String(value).toLowerCase())
      },
      render: (_: unknown, r: ServiceRegionRouteEntry) => <Text strong>{(r.pmNamespace ?? r.namespace ?? '')}/{(r.pmName ?? r.name ?? '')}</Text>,
    },
    {
      title: 'Cluster Ref',
      key: 'clusterRef',
      filterDropdown: searchDropdown('Search cluster ref'),
      filterIcon,
      onFilter: (value: React.Key | boolean, r: ServiceRegionRouteEntry) => {
        const ref = r.clusterPmRef ?? r.clusterRef
        if (!ref) return false
        return `${ref.namespace}/${ref.name}`.toLowerCase().includes(String(value).toLowerCase())
      },
      render: (_: unknown, r: ServiceRegionRouteEntry) => (
        (() => {
          // Defensive: broken-ref entries from controller-side wire may omit clusterRef.
          const ref = r.clusterPmRef ?? r.clusterRef
          return ref ? <Text>{ref.namespace}/{ref.name}</Text> : <Text type="secondary">—</Text>
        })()
      ),
    },
    {
      title: t('center.regionRoute.regions'),
      key: 'regions',
      render: (_: unknown, r: ServiceRegionRouteEntry) => (
        <Space direction="vertical" size={4}>
          {r.regions.map((region) => (
            <Tag key={region.name} color={region.failoverTo ? 'orange' : 'green'}>
              {region.name}{region.failoverTo ? ` → ${region.failoverTo}` : ''}
            </Tag>
          ))}
        </Space>
      ),
    },
    {
      title: t('center.regionRoute.failoverBtn'),
      key: 'actions',
      render: (_: unknown, r: ServiceRegionRouteEntry) => <RowActions entry={r} />,
    },
  ], [t])

  return (
    <div>
      <PageHeader
        title="ServiceRegionRoute"
        subtitle={t('page.subtitle.regionRouteService')}
        actions={<Button icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>}
      />
      <AutoComplete
        placeholder={t('center.regionRoute.pmName')}
        value={filter}
        onChange={setFilter}
        options={filterOptions}
        style={{ width: 300, marginBottom: 16 }}
        allowClear
      />
      {isLoading ? (
        <Spin size="large" style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', minHeight: 300 }} />
      ) : filteredItems.length === 0 ? (
        <Empty description={t('center.regionRoute.noData')} />
      ) : (
        <Table
          dataSource={filteredItems}
          columns={columns}
          rowKey={(r) => `${(r.pmNamespace ?? r.namespace ?? '')}/${(r.pmName ?? r.name ?? '')}`}
          pagination={{ pageSize: 10, showTotal: (n) => t('table.totalItems', { n }) }}
          expandable={{
            expandedRowRender: (record) => <ExpandedDetail entry={record} />,
          }}
        />
      )}
    </div>
  )
}
