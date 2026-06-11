import { useState } from 'react'
import { Table, Button, Space, Input, Tag, Badge, Modal, message } from 'antd'
import { ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import SimpleResourceEditor from '@/components/ResourceEditor/common/SimpleResourceEditor'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'

const { Search } = Input

const EndpointSliceList = () => {
  const t = useT()
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()
  const [searchText, setSearchText] = useState('')
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('view')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)

  const {
    items: epSlices,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('endpointslice', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: ({ ns, name }: { ns: string; name: string }) =>
      resourceApi.delete('endpointslice', ns, name),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'endpointslice'] })
    },
  })

  const filtered = epSlices.filter((r) => {
    const s = searchText.toLowerCase()
    const svcName = r.metadata.labels?.['kubernetes.io/service-name'] || ''
    return r.metadata.name.toLowerCase().includes(s) || r.metadata.namespace?.toLowerCase().includes(s) ||
      svcName.toLowerCase().includes(s)
  })

  const getReadyCount = (r: K8sResource) => {
    const endpoints: any[] = (r as any)['endpoints'] || []
    return endpoints.filter((e) => e.conditions?.ready).length
  }

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode)
    setSelectedResource(resource || null)
    setEditorVisible(true)
  }

  const handleSubmit = async (yamlContent: string) => {
    if (editorMode === 'create') {
      await resourceApi.create('endpointslice', 'default', yamlContent)
      message.success(t('msg.createOk'))
    } else if (editorMode === 'edit' && selectedResource) {
      await resourceApi.update('endpointslice', selectedResource.metadata.namespace || 'default', selectedResource.metadata.name, yamlContent)
      message.success(t('msg.updateOk'))
    }
    setEditorVisible(false)
    queryClient.invalidateQueries({ queryKey: ['resource-list', 'endpointslice'] })
  }

  const handleDelete = (r: K8sResource) => {
    Modal.confirm({
      title: t('confirm.deleteTitle'),
      content: t('confirm.deleteMsg', { name: r.metadata.name }),
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate({ ns: r.metadata.namespace || 'default', name: r.metadata.name }),
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
      items: epSlices,
    }),
    {
      title: t('col.assocService'), key: 'service',
      render: (_: any, r: K8sResource) => {
        const svc = r.metadata.labels?.['kubernetes.io/service-name']
        return svc ? <Tag color="blue">{svc}</Tag> : '-'
      },
    },
    {
      title: t('col.endpoints'), key: 'endpoints',
      render: (_: any, r: K8sResource) => {
        const total = ((r as any)['endpoints'] || []).length
        const ready = getReadyCount(r)
        return (
          <Space>
            <Badge color="green" text={`${ready} Ready`} />
            <span>/ {total} Total</span>
          </Space>
        )
      },
    },
    {
      title: t('col.actions'), key: 'actions', width: 200,
      render: (_: any, r: K8sResource) => (
        <Space size="small">
          <Button size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', r)}>{t('btn.view')}</Button>
          <Button size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', r)}>{t('btn.edit')}</Button>
          <Button size="small" danger icon={<DeleteOutlined />} onClick={() => handleDelete(r)}>{t('btn.delete')}</Button>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="EndpointSlice"
        subtitle={t('page.subtitle.endpointSlice')}
        actions={
          <>
            <Button icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</Button>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
          style={{ width: 260 }} allowClear />
      </div>

      {searchText && (
        <SearchScopeHint loaded={epSlices.length} hasNext={hasNextPage ?? false} />
      )}

      <Table rowKey={(r) => `${r.metadata.namespace ?? ''}/${r.metadata.name}`} columns={columns}
        dataSource={filtered} loading={isLoading}
        size="middle"
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !hasNextPage,
          showTotal: (n) =>
            hasNextPage ? t('table.loadedMore', { n }) : t('table.totalItems', { n }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= epSlices.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />
      <SimpleResourceEditor visible={editorVisible} mode={editorMode} resource={selectedResource} title="EndpointSlice"
        onClose={() => setEditorVisible(false)} onSubmit={handleSubmit} />
    </div>
  )
}

export default EndpointSliceList
