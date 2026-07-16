import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Space, Input, Tag, Modal, message } from 'antd'
import { ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { batchDeleteFailureKeys, resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import ServiceEditor from '@/components/ResourceEditor/Service/ServiceEditor'
import type { ServiceResource } from '@/utils/service'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import { useControllerAccess } from '@/hooks/useControllerAccess'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceBatchDeleteConfirmProps, resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'

const { Search } = Input

const ServiceList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()
  const access = useControllerAccess(controllerId ?? null)
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('view')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)

  const {
    items: services,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('service', {
    namespaced: true,
    scope: controllerId ?? null,
    enabled: access.canResource('service', 'list'),
  })

  const deleteMutation = useMutation({
    mutationFn: ({ ns, name, resourceVersion }: { ns: string; name: string; resourceVersion: string }) =>
      resourceApi.delete(mutationTarget, 'service', ns, name, resourceVersion),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'service'] })
    },
  })
  const batchDeleteMutation = useMutation({
    mutationFn: (resources: Array<{namespace:string;name:string;resourceVersion:string}>) => resourceApi.batchDelete(mutationTarget, 'service', resources),
    onSuccess: () => { message.success(t('msg.batchDeleteOk',{n:selectedRowKeys.length})); setSelectedRowKeys([]); queryClient.invalidateQueries({queryKey:['resource-list','service']}) },
    onError: (error: unknown) => {
      const failedKeys = batchDeleteFailureKeys(error)
      if (failedKeys) {
        setSelectedRowKeys(failedKeys)
        void queryClient.invalidateQueries()
      }
      message.error(error instanceof Error ? error.message : String(error))
    },
  })

  const filtered = services.filter((r) => {
    const s = searchText.toLowerCase()
    return r.metadata.name.toLowerCase().includes(s) || r.metadata.namespace?.toLowerCase().includes(s)
  })

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode)
    setSelectedResource(resource || null)
    setEditorVisible(true)
  }

  const handleSubmit = async (yamlContent: string, submitted: ServiceResource) => {
    if (editorMode === 'create') {
      await resourceApi.create(mutationTarget, 'service', submitted.metadata.namespace, yamlContent)
      message.success(t('msg.createOk'))
    } else if (editorMode === 'edit' && selectedResource) {
      await resourceApi.update(mutationTarget, 'service', selectedResource.metadata.namespace || 'default', selectedResource.metadata.name, yamlContent)
      message.success(t('msg.updateOk'))
    }
    setEditorVisible(false)
    queryClient.invalidateQueries({ queryKey: ['resource-list', 'service'] })
  }

  const handleDelete = (r: K8sResource) => {
    Modal.confirm({
      ...resourceDeleteConfirmProps,
      title: t('confirm.deleteTitle'),
      content: t('confirm.deleteMsg', { name: r.metadata.name }),
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate({ ns: r.metadata.namespace || 'default', name: r.metadata.name, resourceVersion: r.metadata.resourceVersion! }),
    })
  }
  const handleBatchDelete = () => {
    const selected=services.filter((r)=>selectedRowKeys.includes(`${r.metadata.namespace}/${r.metadata.name}`)).map((r)=>({namespace:r.metadata.namespace!,name:r.metadata.name,resourceVersion:r.metadata.resourceVersion!}))
    Modal.confirm({...resourceBatchDeleteConfirmProps,title:t('confirm.batchDeleteTitle'),content:`${t('confirm.batchDeleteMsg',{n:selected.length})} ${t('confirm.deleteIrreversible')}`,okText:t('confirm.okText'),okType:'danger',cancelText:t('btn.cancel'),onOk:()=>batchDeleteMutation.mutate(selected)})
  }

  const columns = [
    ...getResourceMetaColumns<K8sResource>({
      namespaced: true,
      titles: {
        name: t('col.name'),
        namespace: t('col.namespace'),
        age: t('col.age'),
      },
      items: services,
    }),
    { title: t('col.type'), key: 'type',
      render: (_: any, r: K8sResource) => <Tag color="blue">{r.spec?.type || 'ClusterIP'}</Tag> },
    {
      title: t('col.ports'), key: 'ports',
      render: (_: any, r: K8sResource) => (
        <Space wrap>
          {(r.spec?.ports || []).slice(0, 3).map((p: any) => (
            <Tag key={p.port}>{p.name ? `${p.name}:` : ''}{p.port}/{p.protocol || 'TCP'}</Tag>
          ))}
        </Space>
      ),
    },
    {
      title: t('col.actions'), key: 'actions', width: 200,
      render: (_: any, r: K8sResource) => (
        <Space size="small">
          <PermissionAwareButton resourceKind="service" resourceVerb="get" size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', r)}>{t('btn.view')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="service" resourceVerb="update" size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', r)}>{t('btn.edit')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="service" resourceVerb="delete" size="small" danger icon={<DeleteOutlined />} onClick={() => handleDelete(r)}>{t('btn.delete')}</PermissionAwareButton>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="Service"
        subtitle={t('page.subtitle.service')}
        actions={
          <>
            <PermissionAwareButton resourceKind="service" resourceVerb="list" icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</PermissionAwareButton>
            <PermissionAwareButton resourceKind="service" resourceVerb="create" type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</PermissionAwareButton>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search data-testid={resourceActionTestId('service', 'search')} placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
          style={{ width: 240 }} allowClear />
      </div>

      {searchText && (
        <SearchScopeHint loaded={services.length} hasNext={hasNextPage ?? false} />
      )}
      {selectedRowKeys.length>0&&<div style={{marginBottom:16}}><PermissionAwareButton data-testid={resourceActionTestId('service', 'batch-delete')} resourceKind="service" resourceVerb="delete" danger onClick={handleBatchDelete}>{t('btn.batchDelete')}</PermissionAwareButton></div>}

      <Table rowKey={(r) => `${r.metadata.namespace ?? ''}/${r.metadata.name}`} columns={columns}
        dataSource={filtered} loading={isLoading}
        rowSelection={{selectedRowKeys,onChange:setSelectedRowKeys}}
        size="middle"
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !hasNextPage,
          showTotal: (n) =>
            hasNextPage ? t('table.loadedMore', { n }) : t('table.totalItems', { n }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= services.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />
      <ServiceEditor visible={editorVisible} mode={editorMode} resource={selectedResource}
        onClose={() => setEditorVisible(false)} onSubmit={handleSubmit} />
    </div>
  )
}

export default ServiceList
