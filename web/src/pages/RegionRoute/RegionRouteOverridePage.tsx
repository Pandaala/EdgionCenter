import { useMemo, useState } from 'react'
import { App, Alert, AutoComplete, Button, Empty, Popover, Select, Space, Spin, Table, Tag, Tooltip, Typography } from 'antd'
import { ReloadOutlined, WarningOutlined } from '@ant-design/icons'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import PageHeader from '@/components/PageHeader'
import {
  regionRouteApi,
  type CenterRegionRouteOverride,
  type RegionRouteOverrideResource,
} from '@/api/regionRoute'
import { useCan } from '@/utils/permissions'

const { Text } = Typography

export type RegionRouteOverrideScope = 'region' | 'service'

function regions(resource: RegionRouteOverrideResource) {
  return resource.spec.data.config.regions ?? []
}

function normalizedConfig(resource: RegionRouteOverrideResource): string {
  return JSON.stringify({
    enable: resource.spec.enable ?? true,
    config: resource.spec.data.config,
  })
}

export function overrideConsistent(
  row: CenterRegionRouteOverride,
  onlineControllerIds: string[],
): boolean {
  if (!onlineControllerIds.length) return false
  const resources = onlineControllerIds
    .map((controllerId) => row.controllers[controllerId])
  if (resources.some((resource) => !resource)) return false
  return new Set(resources.map((resource) => normalizedConfig(resource!))).size === 1
}

export function overrideMatchesFilters(
  row: CenterRegionRouteOverride,
  namespaceFilter: string,
  nameFilter: string,
): boolean {
  const namespace = namespaceFilter.trim().toLowerCase()
  const name = nameFilter.trim().toLowerCase()
  return (!namespace || row.namespace.toLowerCase().includes(namespace))
    && (!name || row.name.toLowerCase().includes(name))
}

function representativeResource(row: CenterRegionRouteOverride) {
  return Object.entries(row.controllers)
    .sort(([left], [right]) => left.localeCompare(right))[0]?.[1]
}

function FailoverStatus({ resource }: { resource: RegionRouteOverrideResource }) {
  const values = regions(resource)
  if (!values.length) return <Text type="secondary">—</Text>
  return (
    <Space direction="vertical" size={4}>
      {values.map((region) => (
        <Tooltip
          key={region.name}
          title={region.failoverTo
            ? `Traffic from ${region.name} is redirected to ${region.failoverTo}`
            : `Traffic remains in ${region.name}`}
        >
          <Tag color={region.failoverTo ? 'orange' : 'green'}>
            {region.name}{region.failoverTo ? ` → ${region.failoverTo}` : ''}
          </Tag>
        </Tooltip>
      ))}
    </Space>
  )
}

function FailoverEditor({
  scope,
  row,
  resource,
  onDone,
}: {
  scope: RegionRouteOverrideScope
  row: CenterRegionRouteOverride
  resource: RegionRouteOverrideResource
  onDone?: () => void
}) {
  const { message } = App.useApp()
  const queryClient = useQueryClient()
  const values = regions(resource)
  const [pending, setPending] = useState<Record<string, string>>(
    () => Object.fromEntries(values.map((region) => [region.name, region.failoverTo ?? ''])),
  )
  const changed = values.filter(
    (region) => (region.failoverTo ?? '') !== (pending[region.name] ?? ''),
  )
  const mutation = useMutation({
    mutationFn: async () => {
      for (const region of changed) {
        await regionRouteApi.overrideFailover(
          scope,
          row.namespace,
          row.name,
          region.name,
          pending[region.name] ?? '',
        )
      }
      await queryClient.refetchQueries({ queryKey: ['region-route-overrides', scope] })
    },
    onSuccess: () => {
      message.success('Failover updated on every Controller')
      onDone?.()
    },
    onError: (error: Error) => message.error(error.message),
  })

  return (
    <Space direction="vertical" size={8}>
      {values.map((region) => (
        <Space key={region.name}>
          <Text strong style={{ width: 140 }}>{region.name}</Text>
          <Select
            value={pending[region.name] ?? ''}
            disabled={mutation.isPending}
            style={{ width: 180 }}
            onChange={(value) => setPending((current) => ({
              ...current,
              [region.name]: value,
            }))}
            options={[
              { value: '', label: 'No failover' },
              ...values
                .filter((candidate) => candidate.name !== region.name)
                .map((candidate) => ({
                  value: candidate.name,
                  label: candidate.name,
                })),
            ]}
          />
        </Space>
      ))}
      <Button
        type="primary"
        danger={changed.length > 0}
        disabled={!changed.length}
        loading={mutation.isPending}
        onClick={() => mutation.mutate()}
      >
        Apply to all Controllers
      </Button>
    </Space>
  )
}

function FailoverAction({
  scope,
  row,
  resource,
  disabled,
}: {
  scope: RegionRouteOverrideScope
  row: CenterRegionRouteOverride
  resource: RegionRouteOverrideResource
  disabled: boolean
}) {
  const [open, setOpen] = useState(false)
  const button = (
    <Button
      size="small"
      type="primary"
      disabled={disabled}
    >
      Failover
    </Button>
  )

  if (disabled) {
    return (
      <Tooltip title="Synchronize the override before changing failover">
        <span>{button}</span>
      </Tooltip>
    )
  }

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger="click"
      title="Failover configuration"
      content={(
        <div style={{ minWidth: 360 }}>
          <FailoverEditor
            key={normalizedConfig(resource)}
            scope={scope}
            row={row}
            resource={resource}
            onDone={() => setOpen(false)}
          />
        </div>
      )}
    >
      {button}
    </Popover>
  )
}

function SyncOverrideButton({
  scope,
  row,
  onlineControllerIds,
}: {
  scope: RegionRouteOverrideScope
  row: CenterRegionRouteOverride
  onlineControllerIds: string[]
}) {
  const { message } = App.useApp()
  const queryClient = useQueryClient()
  const sources = Object.keys(row.controllers).sort()
  const [source, setSource] = useState(sources[0] ?? '')
  const targets = onlineControllerIds.filter((controllerId) => controllerId !== source)
  const mutation = useMutation({
    mutationFn: () => regionRouteApi.syncOverride(
      scope,
      row.namespace,
      row.name,
      source,
      targets,
    ),
    onSuccess: async () => {
      message.success('Override synchronized')
      await queryClient.refetchQueries({
        queryKey: ['region-route-overrides', scope],
      })
    },
    onError: (error: Error) => message.error(error.message),
  })
  return (
    <Space>
      <Select
        size="small"
        value={source}
        options={sources.map((controllerId) => ({
          value: controllerId,
          label: controllerId,
        }))}
        onChange={setSource}
        style={{ width: 210 }}
      />
      <Button
        size="small"
        loading={mutation.isPending}
        disabled={!source || !targets.length}
        onClick={() => mutation.mutate()}
      >
        Sync to {targets.length}
      </Button>
    </Space>
  )
}

export default function RegionRouteOverridePage({
  scope,
}: {
  scope: RegionRouteOverrideScope
}) {
  const canWrite = useCan('region-routes:write')
  const [namespaceFilter, setNamespaceFilter] = useState('')
  const [nameFilter, setNameFilter] = useState('')
  const query = useQuery({
    queryKey: ['region-route-overrides', scope],
    queryFn: () => regionRouteApi.listOverrides(scope),
    staleTime: 30_000,
  })
  const rows = useMemo(() => query.data?.data ?? [], [query.data])
  const online = query.data?.onlineControllerIds ?? []
  const visible = useMemo(
    () => rows.filter((row) =>
      overrideMatchesFilters(row, namespaceFilter, nameFilter)),
    [nameFilter, namespaceFilter, rows],
  )
  const namespaceOptions = useMemo(
    () => [...new Set(rows.map((row) => row.namespace))]
      .sort()
      .map((value) => ({ value })),
    [rows],
  )
  const nameOptions = useMemo(
    () => [...new Set(rows
      .filter((row) => overrideMatchesFilters(row, namespaceFilter, ''))
      .map((row) => row.name))]
      .sort()
      .map((value) => ({ value })),
    [namespaceFilter, rows],
  )
  const title = scope === 'region'
    ? 'RegionRoute Region Management'
    : 'RegionRoute Service Management'
  const subtitle = scope === 'region'
    ? 'Region-level failover overrides watched from every Controller'
    : 'Service-level failover overrides watched from every Controller'

  return (
    <div>
      <PageHeader
        title={title}
        subtitle={subtitle}
        actions={(
          <Button icon={<ReloadOutlined />} onClick={() => query.refetch()}>
            Refresh
          </Button>
        )}
      />
      {query.isError && (
        <Alert
          type="error"
          showIcon
          message="Failed to load RegionRoute overrides"
          description={(query.error as Error).message}
          style={{ marginBottom: 16 }}
        />
      )}
      <Space wrap style={{ marginBottom: 16 }}>
        <AutoComplete
          value={namespaceFilter}
          allowClear
          onChange={setNamespaceFilter}
          options={namespaceOptions}
          placeholder="Namespace"
          style={{ width: 280 }}
        />
        <AutoComplete
          value={nameFilter}
          allowClear
          onChange={setNameFilter}
          options={nameOptions}
          placeholder="Name"
          style={{ width: 280 }}
        />
      </Space>
      {query.isLoading ? <Spin size="large" /> : visible.length === 0 ? (
        <Empty description={`No ${scope} overrides`} />
      ) : (
        <Table
          rowKey={(row) => `${row.namespace}/${row.name}`}
          dataSource={visible}
          pagination={{ pageSize: 20 }}
          expandable={{
            expandedRowRender: (row) => (
              <Table
                size="small"
                pagination={false}
                rowKey="controllerId"
                dataSource={online.map((controllerId) => ({
                  controllerId,
                  resource: row.controllers[controllerId],
                }))}
                columns={[
                  {
                    title: 'Controller',
                    dataIndex: 'controllerId',
                    render: (value: string) => <Tag color="blue">{value}</Tag>,
                  },
                  {
                    title: 'State',
                    render: (_value, item) => item.resource
                      ? <Tag color={item.resource.spec.enable === false ? 'default' : 'green'}>{item.resource.spec.enable === false ? 'Disabled' : 'Present'}</Tag>
                      : <Tag color="red">Missing</Tag>,
                  },
                  {
                    title: 'Regions',
                    render: (_value, item) => item.resource
                      ? <Space wrap>{regions(item.resource).map((region) => (
                        <Tag key={region.name} color={region.failoverTo ? 'orange' : 'green'}>
                          {region.name}{region.failoverTo ? ` → ${region.failoverTo}` : ''}
                        </Tag>
                      ))}</Space>
                      : '—',
                  },
                ]}
              />
            ),
          }}
          columns={[
            {
              title: 'Namespace / Name',
              render: (_value, row) => <Text strong>{row.namespace}/{row.name}</Text>,
            },
            {
              title: 'Controllers',
              render: (_value, row) => (
                <Tag color={Object.keys(row.controllers).length === online.length ? 'blue' : 'orange'}>
                  {Object.keys(row.controllers).length}/{online.length}
                </Tag>
              ),
            },
            {
              title: 'Consistency',
              render: (_value, row) => overrideConsistent(row, online)
                ? <Tag color="green">Consistent</Tag>
                : (
                  <Space direction="vertical" size={4}>
                    <Tooltip title="The resource is missing or differs on one or more Controllers">
                      <Tag icon={<WarningOutlined />} color="orange">Inconsistent</Tag>
                    </Tooltip>
                    {canWrite && (
                      <SyncOverrideButton
                        scope={scope}
                        row={row}
                        onlineControllerIds={online}
                      />
                    )}
                  </Space>
                ),
            },
            {
              title: 'Failover Status',
              render: (_value, row) => {
                const resource = representativeResource(row)
                if (!resource) return '—'
                return <FailoverStatus resource={resource} />
              },
            },
            {
              title: 'Actions',
              render: (_value, row) => {
                const resource = representativeResource(row)
                if (!resource) return '—'
                return canWrite ? (
                  <FailoverAction
                    scope={scope}
                    row={row}
                    resource={resource}
                    disabled={!overrideConsistent(row, online)}
                  />
                ) : (
                  <Text type="secondary">Read only</Text>
                )
              },
            },
          ]}
        />
      )}
    </div>
  )
}
