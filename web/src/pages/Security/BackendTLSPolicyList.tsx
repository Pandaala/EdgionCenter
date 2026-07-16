import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Space, Input, Tag, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { batchDeleteFailureKeys, resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import BackendTLSPolicyEditor from '@/components/ResourceEditor/BackendTLSPolicy/BackendTLSPolicyEditor'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { useControllerAccess } from '@/hooks/useControllerAccess'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceBatchDeleteConfirmProps, resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'

const { Search } = Input

const BackendTLSPolicyList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()
  const access = useControllerAccess(controllerId ?? null)

  const {
    items: policies,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('backendtlspolicy', {
    namespaced: true,
    scope: controllerId ?? null,
    enabled: access.canResource('backendtlspolicy', 'list'),
  })

  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name, resourceVersion }: { namespace: string; name: string; resourceVersion: string }) =>
      resourceApi.delete(mutationTarget, 'backendtlspolicy', namespace, name, resourceVersion),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'backendtlspolicy'] })
    },
  })
  const batchDeleteMutation = useMutation({
    mutationFn: (resources: Array<{ namespace: string; name: string; resourceVersion: string }>) =>
      resourceApi.batchDelete(mutationTarget, 'backendtlspolicy', resources),
    onSuccess: () => {
      message.success(t('msg.batchDeleteOk', { n: selectedRowKeys.length }))
      setSelectedRowKeys([])
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'backendtlspolicy'] })
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

  const filtered = policies.filter((r) => {
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
  const handleBatchDelete=()=>{const selected=policies.filter((r)=>selectedRowKeys.includes(`${r.metadata.namespace}/${r.metadata.name}`)).map((r)=>({namespace:r.metadata.namespace!,name:r.metadata.name,resourceVersion:r.metadata.resourceVersion!}));Modal.confirm({...resourceBatchDeleteConfirmProps,title:t('confirm.batchDeleteTitle'),content:`${t('confirm.batchDeleteMsg',{n:selected.length})} ${t('confirm.deleteIrreversible')}`,okText:t('confirm.okText'),okType:'danger',cancelText:t('btn.cancel'),onOk:()=>batchDeleteMutation.mutate(selected)})}

  const columns = [
    ...getResourceMetaColumns<K8sResource>({
      namespaced: true,
      titles: {
        name: t('col.name'),
        namespace: t('col.namespace'),
        age: t('col.age'),
      },
      items: policies,
    }),
    {
      title: t('col.targetService'), key: 'target',
      render: (_: any, r: K8sResource) => (
        <Space wrap>
          {(r.spec?.targetRefs || []).map((ref: any, i: number) => (
            <Tag key={i} color="blue">{ref.name}</Tag>
          ))}
        </Space>
      ),
    },
    {
      title: t('col.hostname'), key: 'hostname',
      render: (_: any, r: K8sResource) => r.spec?.validation?.hostname || '-',
    },
    { title: t('col.status'), key: 'status', render: (_: unknown, r: K8sResource) => <ResourceConditions status={r.status} compact /> },
    {
      title: t('col.actions'), key: 'actions', width: 160,
      render: (_: any, r: K8sResource) => (
        <Space>
          <PermissionAwareButton resourceKind="backendtlspolicy" resourceVerb="get" size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', r)}>{t('btn.view')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="backendtlspolicy" resourceVerb="update" size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', r)}>{t('btn.edit')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="backendtlspolicy" resourceVerb="delete" size="small" danger icon={<DeleteOutlined />}
            onClick={() => handleDelete(r.metadata.namespace!, r.metadata.name, r.metadata.resourceVersion!)}>{t('btn.delete')}</PermissionAwareButton>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="BackendTLSPolicy"
        subtitle={t('page.subtitle.backendTls')}
        actions={
          <>
            <PermissionAwareButton resourceKind="backendtlspolicy" resourceVerb="list" icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</PermissionAwareButton>
            <PermissionAwareButton resourceKind="backendtlspolicy" resourceVerb="create" type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</PermissionAwareButton>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search data-testid={resourceActionTestId('backendtlspolicy', 'search')} placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
          style={{ width: 240 }} allowClear />
      </div>

      {searchText && (
        <SearchScopeHint loaded={policies.length} hasNext={hasNextPage ?? false} />
      )}
      {selectedRowKeys.length>0&&<div style={{marginBottom:16}}><PermissionAwareButton data-testid={resourceActionTestId('backendtlspolicy', 'batch-delete')} resourceKind="backendtlspolicy" resourceVerb="delete" danger onClick={handleBatchDelete}>{t('btn.batchDelete')}</PermissionAwareButton></div>}

      <Table rowKey={(r) => `${r.metadata.namespace ?? ''}/${r.metadata.name}`}
        columns={columns} dataSource={filtered} loading={isLoading}
        rowSelection={{selectedRowKeys,onChange:setSelectedRowKeys}}
        size="middle"
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !hasNextPage,
          showTotal: (n) =>
            hasNextPage ? t('table.loadedMore', { n }) : t('table.totalItems', { n }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= policies.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />
      <BackendTLSPolicyEditor visible={editorVisible} mode={editorMode} resource={selectedResource}
        onClose={() => setEditorVisible(false)} />
    </div>
  )
}

export default BackendTLSPolicyList
