import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Button, Space, Input, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { batchDeleteFailureKeys, resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import type { HTTPRoute } from '@/types/gateway-api'
import HTTPRouteEditor from '@/components/ResourceEditor/HTTPRoute/HTTPRouteEditor'
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

const HTTPRouteList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<HTTPRoute | null>(null)
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  // Fetch HTTPRoutes — cursor-paginated via useResourceList
  const {
    items: routes,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('httproute', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  // Delete mutation
  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name, resourceVersion }: { namespace: string; name: string; resourceVersion: string }) =>
      resourceApi.delete(mutationTarget, 'httproute', namespace, name, resourceVersion),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'httproute'] })
    },
  })

  // Batch delete mutation
  const batchDeleteMutation = useMutation({
    mutationFn: (resources: Array<{ namespace: string; name: string; resourceVersion: string }>) =>
      resourceApi.batchDelete(mutationTarget, 'httproute', resources),
    onSuccess: () => {
      message.success(t('msg.batchDeleteOk', { n: selectedRowKeys.length }))
      setSelectedRowKeys([])
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'httproute'] })
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

  // Filter routes by search text
  const filteredRoutes = routes.filter((route) => {
    const searchLower = searchText.toLowerCase()
    return (
      route.metadata.name.toLowerCase().includes(searchLower) ||
      route.metadata.namespace?.toLowerCase().includes(searchLower)
    )
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
    const selectedResources = filteredRoutes
      .filter((route) => selectedRowKeys.includes(`${route.metadata.namespace}/${route.metadata.name}`))
      .map((route) => ({
        namespace: route.metadata.namespace!,
        name: route.metadata.name,
        resourceVersion: route.metadata.resourceVersion!,
      }))

    Modal.confirm({
      ...resourceBatchDeleteConfirmProps,
      title: t('confirm.batchDeleteTitle'),
      content: `${t('confirm.batchDeleteMsg', { n: selectedResources.length })} ${t('confirm.deleteIrreversible')}`,
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => batchDeleteMutation.mutate(selectedResources),
    })
  }

  const handleCreate = () => {
    setEditorMode('create')
    setSelectedResource(null)
    setEditorVisible(true)
  }

  const handleView = (record: K8sResource) => {
    setEditorMode('view')
    setSelectedResource(record as unknown as HTTPRoute)
    setEditorVisible(true)
  }

  const handleEdit = (record: K8sResource) => {
    setEditorMode('edit')
    setSelectedResource(record as unknown as HTTPRoute)
    setEditorVisible(true)
  }

  const handleEditorClose = () => {
    setEditorVisible(false)
    setSelectedResource(null)
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
      title: t('col.status'),
      key: 'status',
      render: (_: unknown, record: K8sResource) => <ResourceConditions status={record.status} compact />,
    },
    {
      title: t('col.actions'),
      key: 'actions',
      render: (_: any, record: K8sResource) => (
        <Space>
          <Button data-testid={resourceActionTestId('httproute', 'row-view')} type="link" icon={<EyeOutlined />} size="small" onClick={() => handleView(record)}>
            {t('btn.view')}
          </Button>
          <Button data-testid={resourceActionTestId('httproute', 'row-edit')} type="link" icon={<EditOutlined />} size="small" onClick={() => handleEdit(record)}>
            {t('btn.edit')}
          </Button>
          <Button
            data-testid={resourceActionTestId('httproute', 'row-delete')}
            type="link"
            danger
            icon={<DeleteOutlined />}
            size="small"
            onClick={() => handleDelete(record.metadata.namespace!, record.metadata.name, record.metadata.resourceVersion!)}
          >
            {t('btn.delete')}
          </Button>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="HTTPRoute"
        subtitle={t('page.subtitle.httpRoute')}
        actions={
          <>
            <Button data-testid={resourceActionTestId('httproute', 'refresh')} icon={<ReloadOutlined />} onClick={() => refetch()}>
              {t('btn.refresh')}
            </Button>
            <Button data-testid={resourceActionTestId('httproute', 'create')} type="primary" icon={<PlusOutlined />} onClick={handleCreate}>
              {t('btn.create')}
            </Button>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search
          data-testid={resourceActionTestId('httproute', 'search')}
          placeholder={t('ph.searchNameNs')}
          allowClear
          style={{ width: 300 }}
          value={searchText}
          onChange={(e) => setSearchText(e.target.value)}
        />
      </div>

      {searchText && (
        <SearchScopeHint loaded={routes.length} hasNext={hasNextPage ?? false} />
      )}

      {selectedRowKeys.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <Space>
            <span>{t('status.selected', { n: selectedRowKeys.length })}</span>
            <Button data-testid={resourceActionTestId('httproute', 'batch-delete')} danger onClick={handleBatchDelete}>
              {t('btn.batchDelete')}
            </Button>
          </Space>
        </div>
      )}

      <Table
        rowSelection={{
          selectedRowKeys,
          onChange: setSelectedRowKeys,
        }}
        columns={columns}
        dataSource={filteredRoutes}
        loading={isLoading}
        rowKey={(record) => `${record.metadata.namespace ?? ''}/${record.metadata.name}`}
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

      <HTTPRouteEditor
        visible={editorVisible}
        mode={editorMode}
        resource={selectedResource as any}
        onClose={handleEditorClose}
      />
    </div>
  )
}

export default HTTPRouteList
