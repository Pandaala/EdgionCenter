import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Space, Input, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import EdgionConfigDataEditor from '@/components/ResourceEditor/EdgionConfigData/EdgionConfigDataEditor'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'
import ResourceConditions from '@/components/resource/ResourceConditions'

const { Search } = Input

const EdgionConfigDataList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [searchText, setSearchText] = useState('')
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  const {
    items: configDataItems,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('edgionconfigdata', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name, resourceVersion }: { namespace: string; name: string; resourceVersion: string }) =>
      resourceApi.delete(mutationTarget, 'edgionconfigdata', namespace, name, resourceVersion),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionconfigdata'] })
    },
  })

  const filtered = configDataItems.filter((r) => {
    const s = searchText.toLowerCase()
    return r.metadata.name.toLowerCase().includes(s) || r.metadata.namespace?.toLowerCase().includes(s)
  })

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode); setSelectedResource(resource || null); setEditorVisible(true)
  }

  const columns = [
    ...getResourceMetaColumns<K8sResource>({
      namespaced: true,
      titles: {
        name: t('col.name'),
        namespace: t('col.namespace'),
        age: t('col.age'),
      },
      items: configDataItems,
    }),
    { title: t('col.type'), key: 'type', render: (_: any, r: K8sResource) => r.spec?.data?.type || '-' },
    { title: t('col.status'), key: 'status', render: (_: unknown, r: K8sResource) => <ResourceConditions status={r.status} compact /> },
    {
      title: t('col.actions'), key: 'actions', width: 160,
      render: (_: any, record: K8sResource) => (
        <Space>
          <PermissionAwareButton resourceKind="edgionconfigdata" resourceVerb="get" size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', record)}>{t('btn.view')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="edgionconfigdata" resourceVerb="update" size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', record)}>{t('btn.edit')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="edgionconfigdata" resourceVerb="delete" size="small" danger icon={<DeleteOutlined />}
            onClick={() => Modal.confirm({
              ...resourceDeleteConfirmProps,
              title: t('confirm.deleteTitle'), content: t('confirm.deleteMsg', { name: record.metadata.name }),
              okText: t('confirm.okText'), okType: 'danger', cancelText: t('btn.cancel'),
              onOk: () => deleteMutation.mutate({
                namespace: record.metadata.namespace!, name: record.metadata.name, resourceVersion: record.metadata.resourceVersion!,
              }),
            })}>{t('btn.delete')}</PermissionAwareButton>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="EdgionConfigData"
        subtitle={t('page.subtitle.edgionConfigData')}
        actions={
          <>
            <PermissionAwareButton resourceKind="edgionconfigdata" resourceVerb="list" icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</PermissionAwareButton>
            <PermissionAwareButton resourceKind="edgionconfigdata" resourceVerb="create" type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</PermissionAwareButton>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search data-testid={resourceActionTestId('edgionconfigdata', 'search')} placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
          style={{ width: 240 }} allowClear />
      </div>

      {searchText && (
        <SearchScopeHint loaded={configDataItems.length} hasNext={hasNextPage ?? false} />
      )}

      <Table rowKey={(r) => `${r.metadata.namespace ?? ''}/${r.metadata.name}`}
        columns={columns} dataSource={filtered} loading={isLoading}
        size="middle"
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !hasNextPage,
          showTotal: (n) =>
            hasNextPage ? t('table.loadedMore', { n }) : t('table.totalItems', { n }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= configDataItems.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />
      <EdgionConfigDataEditor visible={editorVisible} mode={editorMode} resource={selectedResource}
        onClose={() => setEditorVisible(false)} />
    </div>
  )
}

export default EdgionConfigDataList
