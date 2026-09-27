import { useMemo, useState } from 'react'
import { App, Alert, AutoComplete, Button, Empty, Popover, Select, Space, Spin, Table, Tag, Tooltip, Typography } from 'antd'
import { ReloadOutlined, WarningOutlined } from '@ant-design/icons'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import PageHeader from '@/components/PageHeader'
import {
  regionRouteApi,
  type CenterRegionRouteOverride,
  type RegionRouteOverrideResource,
  type WriteOutcomeSummary,
} from '@/api/regionRoute'
import { isOutcomeFailure } from '@/api/writeOutcome'
import { WriteOutcomeList, type WriteOutcomeItem } from '@/components/WriteOutcome/WriteOutcomeTag'
import { useCan } from '@/utils/permissions'
import { useT } from '@/i18n'

const { Text } = Typography

/**
 * Summarize every region's current `failoverTo` from a write outcome's
 * `observed` document (the full RegionRouteOverride resource as last seen in
 * the local watch cache). Used to satisfy the `superseded` outcome's
 * requirement to show what is actually in effect now.
 */
export function describeObservedRegions(observed: unknown): string {
  const regions = (observed as { spec?: { data?: { config?: { regions?: unknown } } } } | undefined)
    ?.spec?.data?.config?.regions
  if (!Array.isArray(regions) || regions.length === 0) return ''
  return regions
    .map((region: { name?: string; failoverTo?: string }) =>
      region?.failoverTo ? `${region.name} → ${region.failoverTo}` : `${region?.name}`)
    .join(', ')
}

/**
 * Flatten one `WriteOutcomeSummary` per applied region into a single list of
 * (region, controller) rows for `WriteOutcomeList`. A failover apply can
 * touch several regions in one submit; the operator needs every controller's
 * outcome for every region, not just the last one.
 */
export function flattenRegionOutcomes(
  results: readonly { region: string; summary: WriteOutcomeSummary }[],
): WriteOutcomeItem[] {
  return results.flatMap(({ region, summary }) =>
    summary.outcomes.map((outcome) => ({
      key: `${region}:${outcome.controllerId}`,
      label: `${region} → ${outcome.controllerId}`,
      outcome,
      describeObserved: describeObservedRegions,
    })),
  )
}

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
  row,
  resource,
  onDone,
}: {
  row: CenterRegionRouteOverride
  resource: RegionRouteOverrideResource
  onDone?: () => void
}) {
  const t = useT()
  const { message } = App.useApp()
  const queryClient = useQueryClient()
  const values = regions(resource)
  const [pending, setPending] = useState<Record<string, string>>(
    () => Object.fromEntries(values.map((region) => [region.name, region.failoverTo ?? ''])),
  )
  const [outcomeItems, setOutcomeItems] = useState<WriteOutcomeItem[]>([])
  const changed = values.filter(
    (region) => (region.failoverTo ?? '') !== (pending[region.name] ?? ''),
  )
  const mutation = useMutation({
    mutationFn: async () => {
      const results: { region: string; summary: WriteOutcomeSummary }[] = []
      for (const region of changed) {
        const summary = await regionRouteApi.overrideFailover(
          row.namespace,
          row.name,
          region.name,
          pending[region.name] ?? '',
        )
        results.push({ region: region.name, summary })
      }
      await queryClient.refetchQueries({ queryKey: ['region-route-overrides'] })
      return results
    },
    onSuccess: (results) => {
      const items = flattenRegionOutcomes(results)
      setOutcomeItems(items)
      const failedCount = items.filter((item) => isOutcomeFailure(item.outcome.state)).length
      const allConverged = items.length > 0 && items.every((item) => item.outcome.state === 'converged')
      if (allConverged) {
        message.success(t('center.regionRoute.failoverUpdateOk'))
        setOutcomeItems([])
        onDone?.()
      } else if (failedCount === items.length) {
        message.error(t('writeOutcome.summary.allFailed'))
      } else if (failedCount === 0) {
        // Everything landed; some target is superseded/accepted/unknown rather
        // than converged. Reporting this as "N landed, 0 failed" reads as a
        // partial failure that never happened.
        message.warning(t('writeOutcome.summary.landedUnconfirmed', { modified: items.length }))
      } else {
        message.warning(t('writeOutcome.summary.mixed', {
          modified: items.length - failedCount,
          failed: failedCount,
        }))
      }
    },
    onError: (error: Error) => message.error(error.message),
  })

  return (
    <Space direction="vertical" size={8} style={{ width: '100%' }}>
      {values.map((region) => (
        <Space key={region.name}>
          <Text strong style={{ width: 140 }}>{region.name}</Text>
          <Select
            data-testid={`region-failover-select-${region.name}`}
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
        data-testid="region-failover-apply"
        type="primary"
        danger={changed.length > 0}
        disabled={!changed.length}
        loading={mutation.isPending}
        onClick={() => mutation.mutate()}
      >
        Apply to all Controllers
      </Button>
      {outcomeItems.length > 0 && (
        <div style={{ borderTop: '1px solid var(--ec-color-border)', paddingTop: 8, width: '100%' }}>
          <WriteOutcomeList items={outcomeItems} />
        </div>
      )}
    </Space>
  )
}

function FailoverAction({
  row,
  resource,
  disabled,
}: {
  row: CenterRegionRouteOverride
  resource: RegionRouteOverrideResource
  disabled: boolean
}) {
  const [open, setOpen] = useState(false)
  const button = (
    <Button
      data-testid="region-failover"
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
  row,
  onlineControllerIds,
}: {
  row: CenterRegionRouteOverride
  onlineControllerIds: string[]
}) {
  const t = useT()
  const { message } = App.useApp()
  const queryClient = useQueryClient()
  const sources = Object.keys(row.controllers).sort()
  const [source, setSource] = useState(sources[0] ?? '')
  const targets = onlineControllerIds.filter((controllerId) => controllerId !== source)
  const [outcomeItems, setOutcomeItems] = useState<WriteOutcomeItem[]>([])
  const mutation = useMutation({
    mutationFn: () => regionRouteApi.syncOverride(
      row.namespace,
      row.name,
      source,
      targets,
    ),
    onSuccess: async (summary) => {
      const items: WriteOutcomeItem[] = summary.outcomes.map((outcome) => ({
        key: outcome.controllerId,
        label: outcome.controllerId,
        outcome,
        describeObserved: describeObservedRegions,
      }))
      setOutcomeItems(items)
      const allConverged = items.length > 0 && items.every((item) => item.outcome.state === 'converged')
      if (allConverged) {
        message.success(t('center.regionRoute.syncOk'))
        setOutcomeItems([])
      } else if (summary.failed === items.length) {
        message.error(t('writeOutcome.summary.allFailed'))
      } else if (summary.failed === 0) {
        // See the matching branch in FailoverEditor: a "0 failed" warning
        // misreports an all-landed-but-not-all-confirmed batch.
        message.warning(t('writeOutcome.summary.landedUnconfirmed', { modified: summary.modified }))
      } else {
        message.warning(t('writeOutcome.summary.mixed', {
          modified: summary.modified,
          failed: summary.failed,
        }))
      }
      await queryClient.refetchQueries({
        queryKey: ['region-route-overrides'],
      })
    },
    onError: (error: Error) => message.error(error.message),
  })
  return (
    <Space direction="vertical" size={8} style={{ width: '100%' }}>
      <Space>
        <Select
          data-testid="region-sync-source"
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
          data-testid="region-sync-apply"
          size="small"
          loading={mutation.isPending}
          disabled={!source || !targets.length}
          onClick={() => mutation.mutate()}
        >
          Sync to {targets.length}
        </Button>
      </Space>
      {outcomeItems.length > 0 && <WriteOutcomeList items={outcomeItems} />}
    </Space>
  )
}

export default function RegionRouteOverridePage() {
  const canWrite = useCan('region-routes:write')
  const [namespaceFilter, setNamespaceFilter] = useState('')
  const [nameFilter, setNameFilter] = useState('')
  const query = useQuery({
    queryKey: ['region-route-overrides'],
    queryFn: () => regionRouteApi.listOverrides(),
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
  const title = 'RegionRoute Override Management'
  const subtitle = 'RegionRoute failover overrides watched from every Controller'

  return (
    <div>
      <PageHeader
        title={title}
        subtitle={subtitle}
        actions={(
          <Button data-testid="region-refresh" icon={<ReloadOutlined />} onClick={() => query.refetch()}>
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
        <Empty description="No overrides" />
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
