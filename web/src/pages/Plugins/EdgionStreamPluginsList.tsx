import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Space, Input, Tag, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { batchDeleteFailureKeys, resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import EdgionStreamPluginsEditor from '@/components/ResourceEditor/EdgionStreamPlugins/EdgionStreamPluginsEditor'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceBatchDeleteConfirmProps, resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'
import ResourceConditions from '@/components/resource/ResourceConditions'

const { Search } = Input

const EdgionStreamPluginsList = () => {
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
    items: streamPlugins,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('edgionstreamplugins', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name, resourceVersion }: { namespace: string; name: string; resourceVersion: string }) =>
      resourceApi.delete(mutationTarget, 'edgionstreamplugins', namespace, name, resourceVersion),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionstreamplugins'] })
    },
  })

  const batchDeleteMutation = useMutation({
    mutationFn: (resources: Array<{ namespace: string; name: string; resourceVersion: string }>) =>
      resourceApi.batchDelete(mutationTarget, 'edgionstreamplugins', resources),
    onSuccess: () => {
      message.success(t('msg.batchDeleteOk', { n: selectedRowKeys.length }))
      setSelectedRowKeys([])
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionstreamplugins'] })
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

  const filtered = streamPlugins.filter((r) => {
    const s = searchText.toLowerCase()
    return r.metadata.name.toLowerCase().includes(s) || r.metadata.namespace?.toLowerCase().includes(s)
  })

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode); setSelectedResource(resource || null); setEditorVisible(true)
  }

  const handleDelete = (namespace: string, name: string, resourceVersion: string) => {
    Modal.confirm({
      ...resourceDeleteConfirmProps,
      title: t('confirm.deleteTitle'), content: t('confirm.deleteMsg', { name }),
      okText: t('confirm.okText'), okType: 'danger', cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate({ namespace, name, resourceVersion }),
    })
  }

  const handleBatchDelete = () => {
    const selected = filtered
      .filter((r) => selectedRowKeys.includes(`${r.metadata.namespace}/${r.metadata.name}`))
      .map((r) => ({ namespace: r.metadata.namespace!, name: r.metadata.name, resourceVersion: r.metadata.resourceVersion! }))
    Modal.confirm({
      ...resourceBatchDeleteConfirmProps,
      title: t('confirm.batchDeleteTitle'), content: `${t('confirm.batchDeleteMsg', { n: selected.length })} ${t('confirm.deleteIrreversible')}`,
      okText: t('confirm.okText'), okType: 'danger', cancelText: t('btn.cancel'),
      onOk: () => batchDeleteMutation.mutate(selected),
    })
  }

  const columns = [
    ...getResourceMetaColumns<K8sResource>({
      namespaced: true,
      titles: {
        name: t('col.name'),
        namespace: t('col.namespace'),
        age: t('col.age'),
      },
      items: streamPlugins,
    }),
    {
      title: t('col.status'), key: 'status', render: (_: unknown, r: K8sResource) => <ResourceConditions status={r.status} compact />,
    },
    {
      title: t('col.plugins'), key: 'plugins',
      render: (_: any, r: K8sResource) => (
        <Space wrap>
          {(r.spec?.plugins || []).map((p: any, i: number) => (
            <Tag key={i} color="cyan">{p.type}</Tag>
          ))}
        </Space>
      ),
    },
    {
      title: t('col.actions'), key: 'actions', width: 160,
      render: (_: any, record: K8sResource) => (
        <Space>
          <PermissionAwareButton resourceKind="edgionstreamplugins" resourceVerb="get" size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', record)}>{t('btn.view')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="edgionstreamplugins" resourceVerb="update" size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', record)}>{t('btn.edit')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="edgionstreamplugins" resourceVerb="delete" size="small" danger icon={<DeleteOutlined />}
            onClick={() => handleDelete(record.metadata.namespace!, record.metadata.name, record.metadata.resourceVersion!)}>{t('btn.delete')}</PermissionAwareButton>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="EdgionStreamPlugins"
        subtitle={t('page.subtitle.streamPlugins')}
        actions={
          <>
            <PermissionAwareButton resourceKind="edgionstreamplugins" resourceVerb="list" icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</PermissionAwareButton>
            <PermissionAwareButton resourceKind="edgionstreamplugins" resourceVerb="create" type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</PermissionAwareButton>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search data-testid={resourceActionTestId('edgionstreamplugins', 'search')} placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
          style={{ width: 240 }} allowClear />
      </div>

      {searchText && (
        <SearchScopeHint loaded={streamPlugins.length} hasNext={hasNextPage ?? false} />
      )}

      {selectedRowKeys.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <Space>
            <span>{t('status.selected', { n: selectedRowKeys.length })}</span>
            <PermissionAwareButton data-testid={resourceActionTestId('edgionstreamplugins', 'batch-delete')} resourceKind="edgionstreamplugins" resourceVerb="delete" danger onClick={handleBatchDelete}>{t('btn.batchDelete')}</PermissionAwareButton>
          </Space>
        </div>
      )}
      <Table rowKey={(r) => `${r.metadata.namespace ?? ''}/${r.metadata.name}`}
        columns={columns} dataSource={filtered} loading={isLoading}
        rowSelection={{ selectedRowKeys, onChange: setSelectedRowKeys }}
        size="middle"
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !hasNextPage,
          showTotal: (n) =>
            hasNextPage ? t('table.loadedMore', { n }) : t('table.totalItems', { n }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= streamPlugins.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />
      <EdgionStreamPluginsEditor visible={editorVisible} mode={editorMode}
        resource={selectedResource as any} onClose={() => setEditorVisible(false)} />
    </div>
  )
}

export default EdgionStreamPluginsList
