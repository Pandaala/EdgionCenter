import { useMemo, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Table, Space, Switch, Select, Tag, Typography, Button, Tooltip, Popconfirm, Empty, Spin, message, Modal, Input, DatePicker,
} from 'antd'
import type { Dayjs } from 'dayjs'
import { ReloadOutlined, PlusOutlined, DeleteOutlined, EyeOutlined, SearchOutlined } from '@ant-design/icons'
import {
  globalConnectionIpRestrictionApi,
  type CenterGlobalIpRestrictionView,
  type ControllerPmEntry,
} from '@/api/globalConnectionIpRestriction'
import CreateModal from './CreateModal'
import DetailModal from './DetailModal'
import PageHeader from '@/components/PageHeader'

const { Text } = Typography

interface FlatRow {
  controllerId: string
  namespace: string
  name: string
  entry: ControllerPmEntry
}

export default function GlobalConnectionIpRestrictionList() {
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [detailTarget, setDetailTarget] = useState<FlatRow | null>(null)
  // Declarative state for enable confirmation — avoids the imperative
  // Modal.confirm API which has been observed to drop the first mutate call.
  const [enableTarget, setEnableTarget] = useState<{ row: FlatRow; next: boolean } | null>(null)
  // Same declarative pattern for active-profile switch confirmation.
  const [profileTarget, setProfileTarget] = useState<{ row: FlatRow; profile: string } | null>(null)

  const { data: response, isLoading, refetch } = useQuery({
    queryKey: ['global-connection-ip-restrictions'],
    queryFn: () => globalConnectionIpRestrictionApi.list(),
    staleTime: 30_000,
  })

  const items: CenterGlobalIpRestrictionView[] = useMemo(() => {
    // response shape: { success: true, data: CenterGlobalIpRestrictionView[] }
    return (response?.data as CenterGlobalIpRestrictionView[]) ?? []
  }, [response])

  const flatRows = useMemo<FlatRow[]>(() => {
    const rows: FlatRow[] = []
    for (const item of items) {
      for (const [controllerId, entry] of Object.entries(item.controllers)) {
        rows.push({
          controllerId,
          namespace: item.namespace,
          name: item.name,
          entry,
        })
      }
    }
    return rows
  }, [items])

  const patchEnableMutation = useMutation({
    mutationFn: ({ ns, name, enable, ctrl }: { ns: string; name: string; enable: boolean; ctrl: string }) =>
      globalConnectionIpRestrictionApi.patchEnable(ns, name, enable, [ctrl]),
    onSuccess: (res, variables) => {
      // res is the full envelope { success, data: FanOutResponse }
      const fanOut = res?.data
      if (fanOut?.failed?.length > 0) {
        message.error(`Enable toggle failed: ${fanOut.failed[0].error ?? 'unknown'}`)
        return
      }
      message.success('Enable toggled')

      // Optimistic local cache update: Center's /api/v1/center/.../list reads from
      // metadata_store which is updated asynchronously via fed_sync watch events.
      // A naive invalidateQueries here refetches before fed_sync has propagated,
      // pulling the stale enable value and overwriting the UI. Patch the cache
      // directly; stale-time-based refetch will reconcile with the real state
      // after fed_sync converges (typically <1s).
      queryClient.setQueryData(['global-connection-ip-restrictions'], (old: { success: boolean; data?: CenterGlobalIpRestrictionView[] } | undefined) => {
        if (!old?.data) return old
        return {
          ...old,
          data: old.data.map((item) => {
            if (item.namespace !== variables.ns || item.name !== variables.name) return item
            const ctrlEntry = item.controllers?.[variables.ctrl]
            if (!ctrlEntry) return item
            return {
              ...item,
              controllers: {
                ...item.controllers,
                [variables.ctrl]: { ...ctrlEntry, enable: variables.enable },
              },
            }
          }),
        }
      })

      // Schedule a delayed invalidate to reconcile with the eventual real state
      // once fed_sync has propagated the watch event to Center's metadata_store.
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ['global-connection-ip-restrictions'] })
      }, 1500)
    },
    onError: (e: Error) => message.error(`Enable toggle error: ${e.message}`),
  })

  const patchActiveProfileMutation = useMutation({
    mutationFn: ({ ns, name, profile, ctrl }: { ns: string; name: string; profile: string; ctrl: string }) =>
      globalConnectionIpRestrictionApi.patchActiveProfile(ns, name, profile, [ctrl]),
    onSuccess: (res, variables) => {
      const fanOut = res?.data
      if (fanOut?.failed?.length > 0) {
        message.error(`Profile switch failed: ${fanOut.failed[0].error ?? 'unknown'}`)
        return
      }
      message.success('Active profile switched')

      // Same optimistic-update + delayed-invalidate pattern as enable toggle —
      // metadata_store is updated via fed_sync watch events, so immediate invalidate
      // would pull stale data and revert the UI.
      queryClient.setQueryData(['global-connection-ip-restrictions'], (old: { success: boolean; data?: CenterGlobalIpRestrictionView[] } | undefined) => {
        if (!old?.data) return old
        return {
          ...old,
          data: old.data.map((item) => {
            if (item.namespace !== variables.ns || item.name !== variables.name) return item
            const ctrlEntry = item.controllers?.[variables.ctrl]
            if (!ctrlEntry) return item
            return {
              ...item,
              controllers: {
                ...item.controllers,
                [variables.ctrl]: { ...ctrlEntry, activeProfile: variables.profile },
              },
            }
          }),
        }
      })

      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ['global-connection-ip-restrictions'] })
      }, 1500)
    },
    onError: (e: Error) => message.error(`Profile switch error: ${e.message}`),
  })

  const deleteMutation = useMutation({
    mutationFn: ({ ns, name, ctrl }: { ns: string; name: string; ctrl: string }) =>
      globalConnectionIpRestrictionApi.delete_(ns, name, [ctrl]),
    onSuccess: () => {
      message.success('Deleted')
      queryClient.invalidateQueries({ queryKey: ['global-connection-ip-restrictions'] })
    },
    onError: (e: Error) => message.error(`Delete error: ${e.message}`),
  })

  // Distinct filter values pulled from current data so dropdowns don't show stale options.
  const controllerOptions = useMemo(
    () => [...new Set(flatRows.map((r) => r.controllerId))].sort().map((v) => ({ text: v, value: v })),
    [flatRows],
  )
  const namespaceOptions = useMemo(
    () => [...new Set(flatRows.map((r) => r.namespace))].sort().map((v) => ({ text: v, value: v })),
    [flatRows],
  )

  // Renders a search-box filter dropdown for substring matches (case-insensitive).
  const stringFilterDropdown = (placeholder: string) => ({ setSelectedKeys, selectedKeys, confirm, clearFilters }: {
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
        style={{ marginBottom: 8, display: 'block', width: 180 }}
      />
      <Space>
        <Button type="primary" size="small" icon={<SearchOutlined />} onClick={() => confirm()}>Search</Button>
        <Button size="small" onClick={() => { clearFilters?.(); confirm() }}>Reset</Button>
      </Space>
    </div>
  )

  // Date-range filter dropdown for the Updated column.
  const dateRangeFilterDropdown = ({ setSelectedKeys, selectedKeys, confirm, clearFilters }: {
    setSelectedKeys: (keys: React.Key[]) => void
    selectedKeys: React.Key[]
    confirm: () => void
    clearFilters?: () => void
  }) => {
    // selectedKeys is encoded as ["startMs|endMs"] — single string so AntD's filter pipeline accepts it.
    const raw = selectedKeys[0] as string | undefined
    const [start, end] = raw ? raw.split('|').map((s) => (s ? Number(s) : undefined)) : [undefined, undefined]
    return (
      <div style={{ padding: 8 }} onKeyDown={(e) => e.stopPropagation()}>
        <DatePicker.RangePicker
          showTime
          value={start && end ? ([{ valueOf: () => start } as Dayjs, { valueOf: () => end } as Dayjs]) : undefined}
          onChange={(range) => {
            if (range && range[0] && range[1]) {
              setSelectedKeys([`${range[0].valueOf()}|${range[1].valueOf()}`])
            } else {
              setSelectedKeys([])
            }
          }}
          style={{ marginBottom: 8, width: 320 }}
        />
        <Space>
          <Button type="primary" size="small" onClick={() => confirm()}>Apply</Button>
          <Button size="small" onClick={() => { clearFilters?.(); confirm() }}>Reset</Button>
        </Space>
      </div>
    )
  }

  const columns = useMemo(() => [
    {
      title: 'Controller',
      dataIndex: 'controllerId',
      key: 'controllerId',
      filters: controllerOptions,
      onFilter: (value: React.Key | boolean, row: FlatRow) => row.controllerId === value,
      render: (v: string) => <Tag color="blue">{v}</Tag>,
    },
    {
      title: 'Namespace',
      dataIndex: 'namespace',
      key: 'namespace',
      filters: namespaceOptions,
      onFilter: (value: React.Key | boolean, row: FlatRow) => row.namespace === value,
    },
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      filterDropdown: stringFilterDropdown('Search name'),
      filterIcon: (filtered: boolean) => <SearchOutlined style={{ color: filtered ? 'var(--ec-color-brand)' : undefined }} />,
      onFilter: (value: React.Key | boolean, row: FlatRow) =>
        row.name.toLowerCase().includes(String(value).toLowerCase()),
      render: (v: string) => <Text strong>{v}</Text>,
    },
    {
      title: 'Enable',
      key: 'enable',
      filters: [
        { text: 'Enabled', value: true },
        { text: 'Disabled', value: false },
      ],
      onFilter: (value: React.Key | boolean, row: FlatRow) => row.entry.enable === value,
      render: (_: unknown, row: FlatRow) => (
        <Switch
          checked={row.entry.enable}
          loading={patchEnableMutation.isPending}
          // Open a declarative Modal instead of Modal.confirm to avoid the imperative-API gotcha
          // where the first onOk call sometimes doesn't reach the mutation.
          onChange={(next) => setEnableTarget({ row, next })}
        />
      ),
    },
    {
      title: 'Active Profile',
      key: 'activeProfile',
      render: (_: unknown, row: FlatRow) => {
        const options = Object.keys(row.entry.profiles).map((k) => ({ label: k, value: k }))
        return (
          <Select
            value={row.entry.activeProfile}
            options={options}
            style={{ minWidth: 160 }}
            loading={patchActiveProfileMutation.isPending}
            // Open a declarative confirmation Modal instead of mutating immediately.
            // Same rationale as the Enable switch: avoid Modal.confirm imperative gotchas.
            onChange={(profile) => {
              if (profile === row.entry.activeProfile) return
              setProfileTarget({ row, profile })
            }}
          />
        )
      },
    },
    {
      title: 'Profiles',
      key: 'profilesCount',
      render: (_: unknown, row: FlatRow) => {
        const names = Object.keys(row.entry.profiles).join(', ')
        return (
          <Tooltip title={names}>
            <Tag>{Object.keys(row.entry.profiles).length}</Tag>
          </Tooltip>
        )
      },
    },
    {
      title: 'Updated',
      key: 'updated',
      sorter: (a: FlatRow, b: FlatRow) => a.entry.lastModified - b.entry.lastModified,
      filterDropdown: dateRangeFilterDropdown,
      onFilter: (value: React.Key | boolean, row: FlatRow) => {
        const [s, e] = String(value).split('|').map(Number)
        if (!s || !e) return true
        return row.entry.lastModified >= s && row.entry.lastModified <= e
      },
      render: (_: unknown, row: FlatRow) =>
        new Date(row.entry.lastModified).toLocaleString(),
    },
    {
      title: 'Hash',
      key: 'hash',
      render: (_: unknown, row: FlatRow) => (
        <Tooltip title={row.entry.contentHash}>
          <Text code>{row.entry.contentHash.slice(0, 8)}</Text>
        </Tooltip>
      ),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, row: FlatRow) => (
        <Space>
          <Button
            size="small"
            icon={<EyeOutlined />}
            onClick={() => setDetailTarget(row)}
          >
            Detail
          </Button>
          <Popconfirm
            title={`Delete PM '${row.namespace}/${row.name}' on ${row.controllerId}?`}
            onConfirm={() =>
              deleteMutation.mutate({ ns: row.namespace, name: row.name, ctrl: row.controllerId })
            }
          >
            <Button size="small" danger icon={<DeleteOutlined />}>
              Delete
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ], [patchEnableMutation, patchActiveProfileMutation, deleteMutation, controllerOptions, namespaceOptions])

  return (
    <div>
      <PageHeader
        title="GlobalConnectionIpRestriction"
        actions={
          <>
            <Button icon={<ReloadOutlined />} onClick={() => refetch()}>Refresh</Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>
              New
            </Button>
          </>
        }
      />
      {isLoading ? (
        <Spin size="large" style={{ display: 'flex', justifyContent: 'center', minHeight: 300 }} />
      ) : flatRows.length === 0 ? (
        <Empty description="No GlobalConnectionIpRestriction PMs yet" />
      ) : (
        <Table
          dataSource={flatRows}
          columns={columns}
          rowKey={(r) => `${r.controllerId}/${r.namespace}/${r.name}`}
          pagination={{ pageSize: 20 }}
        />
      )}
      <CreateModal
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onSuccess={() => queryClient.invalidateQueries({ queryKey: ['global-connection-ip-restrictions'] })}
      />
      {detailTarget && (
        <DetailModal
          open={!!detailTarget}
          namespace={detailTarget.namespace}
          name={detailTarget.name}
          controllerId={detailTarget.controllerId}
          onClose={() => setDetailTarget(null)}
        />
      )}
      <Modal
        open={!!profileTarget}
        title={profileTarget ? `Switch active profile of '${profileTarget.row.namespace}/${profileTarget.row.name}' on ${profileTarget.row.controllerId}?` : ''}
        okText="Confirm"
        okButtonProps={{ loading: patchActiveProfileMutation.isPending }}
        onOk={() => {
          if (!profileTarget) return
          patchActiveProfileMutation.mutate(
            {
              ns: profileTarget.row.namespace,
              name: profileTarget.row.name,
              profile: profileTarget.profile,
              ctrl: profileTarget.row.controllerId,
            },
            { onSettled: () => setProfileTarget(null) },
          )
        }}
        onCancel={() => setProfileTarget(null)}
      >
        {profileTarget && (
          <Text type="secondary">
            From <Text strong>{profileTarget.row.entry.activeProfile}</Text> to <Text strong>{profileTarget.profile}</Text>.
            This action is fan-out to the selected controller.
          </Text>
        )}
      </Modal>
      <Modal
        open={!!enableTarget}
        title={enableTarget ? `${enableTarget.next ? 'Enable' : 'Disable'} '${enableTarget.row.namespace}/${enableTarget.row.name}' on ${enableTarget.row.controllerId}?` : ''}
        okText="Confirm"
        okButtonProps={{ danger: enableTarget ? !enableTarget.next : false, loading: patchEnableMutation.isPending }}
        onOk={() => {
          if (!enableTarget) return
          patchEnableMutation.mutate(
            {
              ns: enableTarget.row.namespace,
              name: enableTarget.row.name,
              enable: enableTarget.next,
              ctrl: enableTarget.row.controllerId,
            },
            { onSettled: () => setEnableTarget(null) },
          )
        }}
        onCancel={() => setEnableTarget(null)}
      >
        {enableTarget && (
          <Text type="secondary">
            Current state: <Text strong>{enableTarget.row.entry.enable ? 'Enabled' : 'Disabled'}</Text>.
            This action is fan-out to the selected controller.
          </Text>
        )}
      </Modal>
    </div>
  )
}
