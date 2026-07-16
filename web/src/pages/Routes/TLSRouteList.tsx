import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Button, Space, Input, Tag, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { batchDeleteFailureKeys, resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import StreamRouteEditor from '@/components/ResourceEditor/StreamRoute/StreamRouteEditor'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceBatchDeleteConfirmProps, resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'

const { Search } = Input

const TLSRouteList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  const {
    items: routes,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('tlsroute', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name, resourceVersion }: { namespace: string; name: string; resourceVersion: string }) =>
      resourceApi.delete(mutationTarget, 'tlsroute', namespace, name, resourceVersion),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'tlsroute'] })
    },
  })

  const batchDeleteMutation = useMutation({
    mutationFn: (resources: Array<{ namespace: string; name: string; resourceVersion: string }>) =>
      resourceApi.batchDelete(mutationTarget, 'tlsroute', resources),
    onSuccess: () => {
      message.success(t('msg.batchDeleteOk', { n: selectedRowKeys.length }))
      setSelectedRowKeys([])
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'tlsroute'] })
    },
    onError: (error: unknown) => {
      const failedKeys = batchDeleteFailureKeys(error)
      if (failedKeys) {
        setSelectedRowKeys(failedKeys)
        void queryClient.invalidateQueries()
      }
      message.error(error instanceof Error ? error.message : String(error))
    },
  })

  const filtered = routes.filter((r) => {
    const s = searchText.toLowerCase()
    return r.metadata.name.toLowerCase().includes(s) || r.metadata.namespace?.toLowerCase().includes(s)
  })

  const handleDelete = (namespace: string, name: string, resourceVersion: string) => {
    Modal.confirm({
      ...resourceDeleteConfirmProps,
      title: t('confirm.deleteTitle'),
      content: t('confirm.deleteMsg', { name }),
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate({ namespace, name, resourceVersion }),
    })
  }

  const handleBatchDelete = () => {
    const selected = filtered
      .filter((r) => selectedRowKeys.includes(`${r.metadata.namespace}/${r.metadata.name}`))
      .map((r) => ({ namespace: r.metadata.namespace!, name: r.metadata.name, resourceVersion: r.metadata.resourceVersion! }))
    Modal.confirm({
      ...resourceBatchDeleteConfirmProps,
      title: t('confirm.batchDeleteTitle'),
      content: `${t('confirm.batchDeleteMsg', { n: selected.length })} ${t('confirm.deleteIrreversible')}`,
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => batchDeleteMutation.mutate(selected),
    })
  }

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode)
    setSelectedResource(resource || null)
    setEditorVisible(true)
  }

  const columns = [
    ...getResourceMetaColumns<K8sResource>({
      namespaced: true,
      titles: {
        name: t('col.name'),
        namespace: t('col.namespace'),
        age: t('col.age'),
      },
      items: routes,
    }),
    {
      title: t('col.sniHostnames'),
      key: 'hostnames',
      render: (_: any, record: K8sResource) => (
        <Space wrap>
          {(record.spec?.hostnames || []).slice(0, 3).map((h: string) => (
            <Tag key={h} color="purple">{h}</Tag>
          ))}
        </Space>
      ),
    },
    { title: t('col.status'), key: 'status', render: (_: unknown, r: K8sResource) => <ResourceConditions status={r.status} compact /> },
    {
      title: t('col.actions'),
      key: 'actions',
      width: 160,
      render: (_: any, record: K8sResource) => (
        <Space>
          <Button data-testid={resourceActionTestId('tlsroute', 'row-view')} size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', record)}>{t('btn.view')}</Button>
          <Button data-testid={resourceActionTestId('tlsroute', 'row-edit')} size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', record)}>{t('btn.edit')}</Button>
          <Button data-testid={resourceActionTestId('tlsroute', 'row-delete')} size="small" danger icon={<DeleteOutlined />}
            onClick={() => handleDelete(record.metadata.namespace!, record.metadata.name, record.metadata.resourceVersion!)}>{t('btn.delete')}</Button>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="TLSRoute"
        subtitle={t('page.subtitle.tlsRoute')}
        actions={
          <>
            <Button data-testid={resourceActionTestId('tlsroute', 'refresh')} icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>
            <Button data-testid={resourceActionTestId('tlsroute', 'create')} type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</Button>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search data-testid={resourceActionTestId('tlsroute', 'search')} placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
          style={{ width: 240 }} allowClear />
      </div>

      {searchText && (
        <SearchScopeHint loaded={routes.length} hasNext={hasNextPage ?? false} />
      )}

      {selectedRowKeys.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <Space>
            <span>{t('status.selected', { n: selectedRowKeys.length })}</span>
            <Button data-testid={resourceActionTestId('tlsroute', 'batch-delete')} danger onClick={handleBatchDelete}>{t('btn.batchDelete')}</Button>
          </Space>
        </div>
      )}

      <Table
        rowKey={(r) => `${r.metadata.namespace ?? ''}/${r.metadata.name}`}
        columns={columns}
        dataSource={filtered}
        loading={isLoading}
        rowSelection={{ selectedRowKeys, onChange: setSelectedRowKeys }}
        size="middle"
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !hasNextPage,
          showTotal: (n) =>
            hasNextPage ? t('table.loadedMore', { n }) : t('table.totalItems', { n }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= routes.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />

      <StreamRouteEditor
        visible={editorVisible}
        mode={editorMode}
        kind="TLSRoute"
        resource={selectedResource as any}
        onClose={() => setEditorVisible(false)}
      />
    </div>
  )
}

export default TLSRouteList
