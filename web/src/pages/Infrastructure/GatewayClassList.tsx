import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Button, Space, Input, Tag, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { clusterResourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import GatewayClassEditor from '@/components/ResourceEditor/GatewayClass/GatewayClassEditor'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'
import ResourceConditions from '@/components/resource/ResourceConditions'

const { Search } = Input

const GatewayClassList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [searchText, setSearchText] = useState('')
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)
  const queryClient = useQueryClient()

  const {
    items: gatewayClasses,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('gatewayclass', { namespaced: false })

  const deleteMutation = useMutation({
    mutationFn: ({ name, resourceVersion }: { name: string; resourceVersion: string }) => clusterResourceApi.delete(mutationTarget, 'gatewayclass', name, resourceVersion),
    onSuccess: () => { message.success(t('msg.deleteOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'gatewayclass'] }) },
  })

  const filtered = gatewayClasses.filter((r) => r.metadata.name.toLowerCase().includes(searchText.toLowerCase()))

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode); setSelectedResource(resource || null); setEditorVisible(true)
  }

  const handleDelete = (name: string, resourceVersion: string) => {
    Modal.confirm({
      ...resourceDeleteConfirmProps,
      title: t('confirm.deleteTitle'), content: t('confirm.deleteMsg', { name }),
      okText: t('confirm.okText'), okType: 'danger', cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate({ name, resourceVersion }),
    })
  }

  const columns = [
    ...getResourceMetaColumns<K8sResource>({
      namespaced: false,
      titles: { name: t('col.name'), namespace: '', age: t('col.age') },
      items: gatewayClasses,
    }),
    { title: t('col.controller'), key: 'controller',
      render: (_: any, r: K8sResource) => <Tag color="blue">{r.spec?.controllerName || '-'}</Tag> },
    { title: t('col.description'), key: 'desc',
      render: (_: any, r: K8sResource) => r.spec?.description || '-' },
    { title: t('col.status'), key: 'status', render: (_: unknown, r: K8sResource) => <ResourceConditions status={r.status} compact /> },
    {
      title: t('col.actions'), key: 'actions', width: 160,
      render: (_: any, r: K8sResource) => (
        <Space>
          <Button data-testid={resourceActionTestId('gatewayclass', 'row-view')} size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', r)}>{t('btn.view')}</Button>
          <Button data-testid={resourceActionTestId('gatewayclass', 'row-edit')} size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', r)}>{t('btn.edit')}</Button>
          <Button data-testid={resourceActionTestId('gatewayclass', 'row-delete')} size="small" danger icon={<DeleteOutlined />} onClick={() => handleDelete(r.metadata.name, r.metadata.resourceVersion!)}>{t('btn.delete')}</Button>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="GatewayClass"
        subtitle={t('page.subtitle.gatewayClass')}
        actions={
          <>
            <Button data-testid={resourceActionTestId('gatewayclass', 'refresh')} icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>
            <Button data-testid={resourceActionTestId('gatewayclass', 'create')} type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</Button>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search data-testid={resourceActionTestId('gatewayclass', 'search')} placeholder={t('ph.searchName')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
          style={{ width: 240 }} allowClear />
      </div>
      {searchText && (
        <SearchScopeHint loaded={gatewayClasses.length} hasNext={hasNextPage ?? false} />
      )}
      <Table
        rowKey={(r) => r.metadata.name}
        columns={columns}
        dataSource={filtered}
        loading={isLoading}
        size="middle"
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !hasNextPage,
          showTotal: (n) =>
            hasNextPage ? t('table.loadedMore', { n }) : t('table.totalItems', { n }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= gatewayClasses.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />
      <GatewayClassEditor
        visible={editorVisible}
        mode={editorMode}
        resource={selectedResource}
        onClose={() => setEditorVisible(false)}
      />
    </div>
  )
}

export default GatewayClassList
