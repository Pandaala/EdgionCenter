import { useMemo, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Table, Space, Tag, Typography, Button, Tooltip, Empty, Spin, message, Modal, Input, Select,
} from 'antd'
import { ReloadOutlined, EyeOutlined, SearchOutlined } from '@ant-design/icons'
import {
  globalConnectionIpRestrictionApi,
  type CenterGirAggregatedView,
  type EffectiveGirView,
  type FanOutResponse,
} from '@/api/globalConnectionIpRestriction'
import { isOutcomeFailure, OUTCOME_MESSAGE_SEVERITY, type WriteOutcome } from '@/api/writeOutcome'
import { outcomeDetailText } from '@/components/WriteOutcome/WriteOutcomeTag'
import DetailModal from './DetailModal'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useCan } from '@/utils/permissions'

const { Text } = Typography

interface FlatRow {
  controllerId: string
  namespace: string
  pluginName: string
  entry: EffectiveGirView
}

/** Find the fan-out result for one controller and return its attached `WriteOutcome`, if any. */
export function resolveWriteOutcome(
  fanOut: Pick<FanOutResponse, 'success' | 'failed'>,
  controllerId: string,
): WriteOutcome | undefined {
  return [...fanOut.success, ...fanOut.failed].find((result) => result.controllerId === controllerId)?.outcome
}

/**
 * Extract the active profile name from a write outcome's `observed` Selector
 * document — used to satisfy the `superseded` outcome's requirement to show
 * what is actually in effect now.
 */
export function describeSelectorActiveProfile(observed: unknown): string {
  const active = (observed as { spec?: { data?: { config?: { active?: unknown } } } } | undefined)
    ?.spec?.data?.config?.active
  return typeof active === 'string' ? active : ''
}

export default function GlobalConnectionIpRestrictionList() {
  const t = useT()
  const queryClient = useQueryClient()
  const canWrite = useCan('ip-restrictions:write')
  const [detailTarget, setDetailTarget] = useState<FlatRow | null>(null)
  // Declarative state for active-profile switch confirmation.
  const [profileTarget, setProfileTarget] = useState<{ row: FlatRow; profile: string } | null>(null)

  const { data: response, isLoading, refetch } = useQuery({
    queryKey: ['global-connection-ip-restrictions'],
    queryFn: () => globalConnectionIpRestrictionApi.list(),
    staleTime: 30_000,
  })

  const items: CenterGirAggregatedView[] = useMemo(() => {
    return (response?.data as CenterGirAggregatedView[]) ?? []
  }, [response])

  const flatRows = useMemo<FlatRow[]>(() => {
    const rows: FlatRow[] = []
    for (const item of items) {
      for (const [controllerId, entry] of Object.entries(item.controllers)) {
        rows.push({
          controllerId,
          namespace: item.namespace,
          pluginName: item.pluginName,
          entry,
        })
      }
    }
    return rows
  }, [items])

  const patchActiveProfileMutation = useMutation({
    mutationFn: ({ ns, name, profile, ctrl }: { ns: string; name: string; profile: string; ctrl: string }) =>
      globalConnectionIpRestrictionApi.patchActiveProfile(ns, name, profile, [ctrl]),
    onSuccess: (res, variables) => {
      const fanOut = res?.data
      const outcome = fanOut ? resolveWriteOutcome(fanOut, variables.ctrl) : undefined

      if (outcome) {
        // The write core reported a state for this controller — six outcomes,
        // not just "did it fail": show the state-specific wording (and, for
        // `superseded`, what is actually in effect now) instead of a generic
        // success/failure toast.
        const text = `${t(`writeOutcome.state.${outcome.state}`)}: ${outcomeDetailText(t, outcome, describeSelectorActiveProfile)}`
        message[OUTCOME_MESSAGE_SEVERITY[outcome.state]](text)
      } else if ((fanOut?.failed?.length ?? 0) > 0) {
        message.error(`Profile switch failed: ${fanOut?.failed[0]?.error ?? 'unknown'}`)
      } else {
        message.success('Active profile switched')
      }

      if (outcome ? isOutcomeFailure(outcome.state) : (fanOut?.failed?.length ?? 0) > 0) {
        // Nothing landed on this controller — leave the cached active
        // profile untouched.
        return
      }
      if (outcome?.state === 'superseded') {
        // The write landed but was immediately overwritten by something
        // else, so the requested profile is NOT what is in effect — an
        // optimistic set to `variables.profile` here would be wrong.
        // Refresh from the read model instead of guessing.
        queryClient.invalidateQueries({ queryKey: ['global-connection-ip-restrictions'] })
        return
      }

      // Optimistic local cache update — metadata_store is updated via fed_sync watch events,
      // so an immediate invalidate would pull stale data and revert the UI.
      queryClient.setQueryData(['global-connection-ip-restrictions'], (old: { success: boolean; data?: CenterGirAggregatedView[] } | undefined) => {
        if (!old?.data) return old
        return {
          ...old,
          data: old.data.map((item) => {
            if (item.namespace !== variables.ns || item.pluginName !== variables.name) return item
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

      // Reconcile without publishing an intermediate stale snapshot. A successful
      // Controller write can take multiple federation ping intervals to arrive in
      // Center's metadata store; invalidating after a fixed short delay would make
      // the optimistic value visibly jump back to the old profile.
      const delays = [1500, 3000, 5000, 8000, 12000]
      const reconcile = async (attempt: number): Promise<void> => {
        await new Promise((resolve) => setTimeout(resolve, delays[attempt]))
        try {
          const fresh = await globalConnectionIpRestrictionApi.list()
          const current = fresh.data
            ?.find((item) => item.namespace === variables.ns && item.pluginName === variables.name)
            ?.controllers?.[variables.ctrl]?.activeProfile

          if (current === variables.profile || attempt === delays.length - 1) {
            queryClient.setQueryData(['global-connection-ip-restrictions'], fresh)
            return
          }
        } catch {
          // A transient reconciliation read must not turn a successful write into
          // a user-visible error. The next bounded attempt can still converge.
        }

        if (attempt + 1 < delays.length) void reconcile(attempt + 1)
      }
      void reconcile(0)
    },
    onError: (e: Error) => message.error(`Profile switch error: ${e.message}`),
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
        data-testid="gcir-search"
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

  const columns = useMemo(() => [
    {
      title: <span data-testid="gcir-controller">Controller</span>,
      dataIndex: 'controllerId',
      key: 'controllerId',
      filters: controllerOptions,
      onFilter: (value: React.Key | boolean, row: FlatRow) => row.controllerId === value,
      render: (v: string) => <Tag color="blue">{v}</Tag>,
    },
    {
      title: <span data-testid="gcir-namespace">Namespace</span>,
      dataIndex: 'namespace',
      key: 'namespace',
      filters: namespaceOptions,
      onFilter: (value: React.Key | boolean, row: FlatRow) => row.namespace === value,
    },
    {
      title: 'Name',
      dataIndex: 'pluginName',
      key: 'pluginName',
      filterDropdown: stringFilterDropdown('Search name'),
      filterIcon: (filtered: boolean) => <SearchOutlined style={{ color: filtered ? 'var(--ec-color-brand)' : undefined }} />,
      onFilter: (value: React.Key | boolean, row: FlatRow) =>
        row.pluginName.toLowerCase().includes(String(value).toLowerCase()),
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
        <Tag color={row.entry.enable ? 'green' : 'default'}>{row.entry.enable ? 'Enabled' : 'Disabled'}</Tag>
      ),
    },
    {
      title: 'Active Profile',
      key: 'activeProfile',
      render: (_: unknown, row: FlatRow) => {
        const options = Object.keys(row.entry.profiles).map((k) => ({ label: k, value: k }))
        return (
          <Select
            data-testid="gcir-profile-select"
            value={row.entry.activeProfile}
            options={options}
            style={{ minWidth: 160 }}
            loading={patchActiveProfileMutation.isPending}
            disabled={!canWrite || row.entry.activeProfileRef?.permitted !== true}
            // Open a declarative confirmation Modal instead of mutating immediately.
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
      title: 'Selector Applied',
      key: 'selectorApplied',
      render: (_: unknown, row: FlatRow) => (
        <Tag color={row.entry.selectorApplied ? 'blue' : 'default'}>
          {row.entry.selectorApplied ? 'Yes' : 'No'}
        </Tag>
      ),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, row: FlatRow) => (
        <Space>
          <Button
            data-testid="gcir-row-open"
            size="small"
            icon={<EyeOutlined />}
            onClick={() => setDetailTarget(row)}
          >
            Detail
          </Button>
        </Space>
      ),
    },
  ], [canWrite, patchActiveProfileMutation, controllerOptions, namespaceOptions])

  return (
    <div>
      <PageHeader
        title={t('center.nav.globalIpLists')}
        actions={
          <Button data-testid="gcir-refresh" icon={<ReloadOutlined />} onClick={() => refetch()}>Refresh</Button>
        }
      />
      {isLoading ? (
        <Spin size="large" style={{ display: 'flex', justifyContent: 'center', minHeight: 300 }} />
      ) : flatRows.length === 0 ? (
        <Empty description={t('globalRules.ipLists.empty')} />
      ) : (
        <Table
          dataSource={flatRows}
          columns={columns}
          rowKey={(r) => `${r.controllerId}/${r.namespace}/${r.pluginName}`}
          pagination={{ pageSize: 20 }}
        />
      )}
      {detailTarget && (
        <DetailModal
          open={!!detailTarget}
          namespace={detailTarget.namespace}
          name={detailTarget.pluginName}
          controllerId={detailTarget.controllerId}
          onClose={() => setDetailTarget(null)}
        />
      )}
      <Modal
        open={!!profileTarget}
        title={profileTarget ? `Switch active profile of '${profileTarget.row.namespace}/${profileTarget.row.pluginName}' on ${profileTarget.row.controllerId}?` : ''}
        okText="Confirm"
        okButtonProps={{ loading: patchActiveProfileMutation.isPending, 'data-testid': 'gcir-profile-confirm' } as React.ComponentProps<typeof Button>}
        cancelButtonProps={{ 'data-testid': 'gcir-profile-cancel' } as React.ComponentProps<typeof Button>}
        onOk={() => {
          if (!profileTarget) return
          patchActiveProfileMutation.mutate(
            {
              ns: profileTarget.row.namespace,
              name: profileTarget.row.pluginName,
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
    </div>
  )
}
