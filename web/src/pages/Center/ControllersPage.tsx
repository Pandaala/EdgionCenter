import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Badge, Button, Input, Modal, Select, Space, Table, Tag, Typography, message } from 'antd'
import { ArrowRightOutlined, ReloadOutlined, SyncOutlined } from '@ant-design/icons'
import { centerApi, type AdminControllerDto, type ControllerSummary } from '@/api/center'
import { useServerInfo } from '@/hooks/useServerInfo'
import { invalidateControllerAccess } from '@/hooks/useControllerAccess'
import { useCan } from '@/utils/permissions'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'
import { reloadOutcomeMessage } from '@/components/WriteOutcome/reloadOutcomeText'
import { reloadErrorMessage } from '@/api/reloadOutcome'

type ControllerRow = ControllerSummary & { lastSeenAt?: number }

const formatLastSeen = (t: ReturnType<typeof useT>, row: ControllerRow) => {
  if (row.lastSeenAt) return new Date(row.lastSeenAt * 1000).toLocaleString()
  const seconds = row.last_seen_secs_ago
  if (seconds == null) return t('center.never')
  if (seconds < 60) return t('center.secsAgo', { n: seconds })
  if (seconds < 3600) return t('center.minsAgo', { n: Math.floor(seconds / 60) })
  return t('center.hoursAgo', { n: Math.floor(seconds / 3600) })
}

export default function ControllersPage() {
  const t = useT()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const canRead = useCan('controllers:read')
  const canWrite = useCan('controllers:write')
  // Entering a Controller only reads through the proxy; writes are gated per
  // action by PermissionAwareButton on the drill-in pages.
  const canProxy = useCan('proxy:read')
  const { data: serverInfo } = useServerInfo()
  const hasHistory = serverInfo?.data?.capabilities?.controllerHistory === true
  const [search, setSearch] = useState('')
  const [cluster, setCluster] = useState<string>()

  const list = useQuery({
    queryKey: ['center-controllers'],
    queryFn: centerApi.listControllers,
    staleTime: 30_000,
  })
  const history = useQuery({
    queryKey: ['center-admin-controllers'],
    queryFn: centerApi.listAdminControllers,
    staleTime: 30_000,
    enabled: hasHistory && canRead,
  })
  const historyById = useMemo(
    () => new Map(
      (history.data?.data ?? []).map(
        (item: AdminControllerDto) => [item.controllerId, item],
      ),
    ),
    [history.data],
  )
  const rows = useMemo(
    () => (list.data?.data ?? []).map((item) => ({
      ...item,
      lastSeenAt: historyById.get(item.controller_id)?.lastSeenAt,
    })),
    [list.data, historyById],
  )
  const clusters = useMemo(() => [...new Set(rows.map((row) => row.cluster))].sort(), [rows])
  const normalizedSearch = search.trim().toLowerCase()
  const filtered = rows.filter((row) => {
    const searchable = `${row.controller_id} ${row.cluster} ${row.env.join(' ')} ${row.tag.join(' ')}`.toLowerCase()
    return (!normalizedSearch || searchable.includes(normalizedSearch))
      && (!cluster || row.cluster === cluster)
  })

  // A reload is reported as a terminal state, never as bare dispatch success.
  // `converged` is a toast; every other state is a modal, because each of them
  // carries something the operator must read (a leader address, a retry delay,
  // or "this may not have finished") that a 3 s toast would lose.
  const reload = useMutation({
    mutationFn: centerApi.reloadController,
    onSuccess: async (outcome, id) => {
      await invalidateControllerAccess(queryClient, id)
      refresh()
      const { level, text } = reloadOutcomeMessage(t, outcome)
      if (level === 'success') {
        message.success(text)
        return
      }
      const show = level === 'error' ? Modal.error : Modal.warning
      show({
        title: t('center.reloadOutcome.title'),
        content: text,
        okButtonProps: { 'data-testid': 'controller-reload-outcome-ok' },
      })
    },
    // The reload request is `_silent`, so the shared interceptor's per-status
    // message never fires — word the transport failure here instead of letting
    // axios's "Request failed with status code 404" reach the operator.
    onError: (error: unknown) => message.error(reloadErrorMessage(error)),
  })
  const remove = useMutation({
    mutationFn: centerApi.deleteAdminController,
    onSuccess: () => {
      message.success(t('center.admin.deleteControllerOk'))
      queryClient.invalidateQueries({ queryKey: ['center-admin-controllers'] })
      queryClient.invalidateQueries({ queryKey: ['center-controllers'] })
    },
  })
  const confirmReload = (id: string) => Modal.confirm({
    title: t('center.reload'),
    content: t('center.reloadConfirm', { name: id }),
    okButtonProps: { 'data-testid': 'controller-reload-confirm' },
    cancelButtonProps: { 'data-testid': 'controller-reload-cancel' },
    // Fire and let the confirm modal close: Center now waits up to 20 s for
    // the Controller to come back, and holding the dialog open for that long
    // would read as a hung dialog. The row's own button keeps spinning until
    // the outcome arrives.
    onOk: () => {
      message.info(t('center.reloadPending', { name: id }))
      reload.mutate(id)
    },
  })
  const confirmDelete = (id: string) => Modal.confirm({
    title: t('confirm.deleteTitle'),
    content: t('center.admin.deleteControllerConfirm', { id }),
    okText: t('confirm.okText'),
    okType: 'danger',
    cancelText: t('btn.cancel'),
    okButtonProps: { 'data-testid': 'controller-delete-confirm' },
    cancelButtonProps: { 'data-testid': 'controller-delete-cancel' },
    onOk: () => remove.mutateAsync(id),
  })
  const refresh = () => {
    list.refetch()
    if (hasHistory && canRead) history.refetch()
  }

  return (
    <div data-testid="controllers-page">
      <PageHeader
        title={t('center.nav.controllers')}
        subtitle={t('center.controllers.subtitle')}
        actions={(
          <Button
            data-testid="controllers-refresh"
            icon={<ReloadOutlined />}
            loading={list.isFetching || history.isFetching}
            onClick={refresh}
          >
            {t('btn.refresh')}
          </Button>
        )}
      />
      <Space wrap style={{ marginBottom: 16 }}>
        <Input.Search
          data-testid="controller-search"
          allowClear
          placeholder={t('center.searchPlaceholder')}
          onChange={(event) => setSearch(event.target.value)}
          style={{ width: 280 }}
        />
        <Select
          data-testid="controller-cluster-filter"
          allowClear
          placeholder={t('center.filterCluster')}
          value={cluster}
          onChange={setCluster}
          options={clusters.map((value) => ({ value, label: value }))}
          style={{ width: 200 }}
        />
      </Space>
      <Table
        rowKey="controller_id"
        loading={list.isLoading || history.isLoading}
        dataSource={filtered}
        pagination={{ pageSize: 20, showTotal: (n) => t('table.totalItems', { n }) }}
        scroll={{ x: 'max-content' }}
        columns={[
          {
            title: t('center.controllerId'),
            dataIndex: 'controller_id',
            render: (value: string) => <Typography.Text strong>{value}</Typography.Text>,
          },
          {
            title: t('center.status'),
            dataIndex: 'online',
            render: (online: boolean) => (
              <Badge
                status={online ? 'success' : 'error'}
                text={online ? t('center.online') : t('center.offline')}
              />
            ),
          },
          {
            title: t('center.cluster'),
            dataIndex: 'cluster',
            render: (value: string) => <Tag color="blue">{value}</Tag>,
          },
          {
            title: t('center.admin.envTag'),
            render: (_: unknown, row: ControllerRow) => (
              <Space wrap size={4}>
                {row.env.map((value) => (
                  <Tag key={`env-${value}`} color="green">{value}</Tag>
                ))}
                {row.tag.map((value) => (
                  <Tag key={`tag-${value}`}>{value}</Tag>
                ))}
              </Space>
            ),
          },
          {
            title: t('center.admin.lastSeen'),
            render: (_: unknown, row: ControllerRow) => formatLastSeen(t, row),
          },
          {
            title: t('center.resourceCount'),
            render: (_: unknown, row: ControllerRow) => {
              if (row.stats_state === 'missing') return '—'
              if (row.stats_state === 'stale') {
                return (
                  <Space size={4}>
                    <span>{row.key_count ?? '—'}</span>
                    <Tag color="orange" data-testid="controller-resources-stale">
                      {t('globalResources.syncState.stale')}
                    </Tag>
                  </Space>
                )
              }
              return row.key_count ?? '—'
            },
          },
          {
            title: t('col.actions'),
            fixed: 'right',
            render: (_: unknown, row: ControllerRow) => (
              <Space>
                {canProxy && (
                  <Button
                    data-testid="controller-enter"
                    size="small"
                    icon={<ArrowRightOutlined />}
                    onClick={() => navigate(`/controller/${row.controller_id.replace(/\//g, '~')}`)}
                  >
                    {t('center.enter')}
                  </Button>
                )}
                {canWrite && (
                  <Button
                    data-testid="controller-reload"
                    size="small"
                    icon={<SyncOutlined />}
                    onClick={() => confirmReload(row.controller_id)}
                    loading={reload.isPending && reload.variables === row.controller_id}
                  >
                    {t('center.reload')}
                  </Button>
                )}
                {hasHistory && canWrite && (
                  <Button
                    data-testid="controller-delete"
                    danger
                    size="small"
                    onClick={() => confirmDelete(row.controller_id)}
                    loading={remove.isPending && remove.variables === row.controller_id}
                  >
                    {t('center.admin.deleteController')}
                  </Button>
                )}
              </Space>
            ),
          },
        ]}
      />
    </div>
  )
}
