import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Input, Modal, Space, Table, Tag, message } from 'antd'
import { DeleteOutlined, EditOutlined, EyeOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import type { EdgionBackendTrafficPolicy } from '@/types/edgion-backend-traffic-policy'
import { batchDeleteFailureKeys, resourceApi } from '@/api/resources'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import ResourceListError from '@/components/resource/ResourceListError'
import ResourceConditions from '@/components/resource/ResourceConditions'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceBatchDeleteConfirmProps, resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'
import PageHeader from '@/components/PageHeader'
import EdgionBackendTrafficPolicyEditor from '@/components/ResourceEditor/EdgionBackendTrafficPolicy/EdgionBackendTrafficPolicyEditor'
import { useT } from '@/i18n'

const { Search } = Input
const RESOURCE_KIND = 'edgionbackendtrafficpolicy' as const

const EdgionBackendTrafficPolicyList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const { controllerId } = useParams<{ controllerId?: string }>()
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editor, setEditor] = useState<{
    visible: boolean
    mode: 'create' | 'edit' | 'view'
    resource?: EdgionBackendTrafficPolicy
  }>({ visible: false, mode: 'create' })
  const queryClient = useQueryClient()
  const list = useResourceList<EdgionBackendTrafficPolicy>(RESOURCE_KIND, {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: async (resources: Array<{ namespace: string; name: string; resourceVersion: string }>) => {
      if (resources.length === 1) await resourceApi.delete(mutationTarget, RESOURCE_KIND, resources[0].namespace, resources[0].name, resources[0].resourceVersion)
      else await resourceApi.batchDelete(mutationTarget, RESOURCE_KIND, resources)
    },
    onSuccess: (_, resources) => {
      message.success(resources.length === 1 ? t('msg.deleteOk') : t('msg.batchDeleteOk', { n: resources.length }))
      setSelectedRowKeys([])
      queryClient.invalidateQueries({ queryKey: ['resource-list', RESOURCE_KIND] })
    },
    onError: (error: unknown) => {
      const failedKeys = batchDeleteFailureKeys(error)
      if (failedKeys) {
        setSelectedRowKeys(failedKeys)
        void queryClient.invalidateQueries()
      }
      const reason = error instanceof Error ? error.message : String(error)
      message.error(t('msg.deleteFailed', { err: reason }))
    },
  })

  const confirmDelete = (resources: Array<{ namespace: string; name: string; resourceVersion: string }>, batch = false) => {
    Modal.confirm({
      ...(batch ? resourceBatchDeleteConfirmProps : resourceDeleteConfirmProps),
      title: batch ? t('confirm.batchDeleteTitle') : t('confirm.deleteTitle'),
      content: `${batch
        ? t('confirm.batchDeleteMsg', { n: resources.length })
        : t('confirm.deleteMsg', { name: resources[0].name })} ${t('confirm.deleteIrreversible')}`,
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate(resources),
    })
  }

  const filtered = list.items.filter((item) => {
    const search = searchText.toLowerCase()
    return item.metadata.name.toLowerCase().includes(search)
      || item.metadata.namespace?.toLowerCase().includes(search)
      || item.spec.targetRefs.some((ref) => ref.name.toLowerCase().includes(search))
  })
  const selected = list.items.filter((item) => selectedRowKeys.includes(`${item.metadata.namespace}/${item.metadata.name}`))

  const columns = [
    ...getResourceMetaColumns<EdgionBackendTrafficPolicy>({
      namespaced: true,
      titles: { name: t('col.name'), namespace: t('col.namespace'), age: t('col.age') },
      items: list.items,
    }),
    {
      title: t('col.targetService'),
      key: 'targets',
      render: (_: unknown, item: EdgionBackendTrafficPolicy) => (
        <Space wrap>{item.spec.targetRefs.map((ref, index) => <Tag color="blue" key={`${ref.name}-${index}`}>{ref.name}</Tag>)}</Space>
      ),
    },
    {
      title: t('col.loadBalancer'),
      key: 'loadBalancer',
      render: (_: unknown, item: EdgionBackendTrafficPolicy) => item.spec.loadBalancer?.type ?? 'RoundRobin',
    },
    {
      title: t('col.trafficPolicy'),
      key: 'features',
      render: (_: unknown, item: EdgionBackendTrafficPolicy) => (
        <Space wrap>
          {item.spec.healthCheck?.active && <Tag color="green">{t('tag.healthCheck')}</Tag>}
          {item.spec.outlierDetection && <Tag color="orange">{t('tag.outlierDetection')}</Tag>}
          {item.spec.upstreamAuthority && <Tag color="purple">{t('tag.upstreamAuthority')}</Tag>}
        </Space>
      ),
    },
    {
      title: t('col.status'),
      key: 'status',
      render: (_: unknown, item: EdgionBackendTrafficPolicy) => <ResourceConditions status={item.status} compact />,
    },
    {
      title: t('col.actions'),
      key: 'actions',
      width: 250,
      render: (_: unknown, item: EdgionBackendTrafficPolicy) => (
        <Space>
          <PermissionAwareButton data-testid={resourceActionTestId(RESOURCE_KIND, 'row-view')} size="small" resourceKind={RESOURCE_KIND} resourceVerb="get" icon={<EyeOutlined />} onClick={() => setEditor({ visible: true, mode: 'view', resource: item })}>{t('btn.view')}</PermissionAwareButton>
          <PermissionAwareButton data-testid={resourceActionTestId(RESOURCE_KIND, 'row-edit')} size="small" resourceKind={RESOURCE_KIND} resourceVerb="update" icon={<EditOutlined />} onClick={() => setEditor({ visible: true, mode: 'edit', resource: item })}>{t('btn.edit')}</PermissionAwareButton>
          <PermissionAwareButton data-testid={resourceActionTestId(RESOURCE_KIND, 'row-delete')} size="small" danger resourceKind={RESOURCE_KIND} resourceVerb="delete" icon={<DeleteOutlined />} onClick={() => confirmDelete([{ namespace: item.metadata.namespace!, name: item.metadata.name, resourceVersion: item.metadata.resourceVersion! }])}>{t('btn.delete')}</PermissionAwareButton>
        </Space>
      ),
    },
  ]

  if (list.error) return <ResourceListError error={list.error} onRetry={list.refetch} />

  return (
    <div data-testid="ebtp-list">
      <PageHeader
        title="EdgionBackendTrafficPolicy"
        subtitle={t('page.subtitle.backendTrafficPolicy')}
        actions={(
          <>
            <Search data-testid={resourceActionTestId(RESOURCE_KIND, 'search')} value={searchText} onChange={(event) => setSearchText(event.target.value)} allowClear placeholder={t('ph.searchNameNsService')} style={{ width: 260 }} />
            <PermissionAwareButton data-testid={resourceActionTestId(RESOURCE_KIND, 'refresh')} icon={<ReloadOutlined />} resourceKind={RESOURCE_KIND} resourceVerb="list" onClick={() => list.refetch()}>{t('btn.refresh')}</PermissionAwareButton>
            <PermissionAwareButton data-testid={resourceActionTestId(RESOURCE_KIND, 'create')} type="primary" icon={<PlusOutlined />} resourceKind={RESOURCE_KIND} resourceVerb="create" onClick={() => setEditor({ visible: true, mode: 'create' })}>{t('btn.create')}</PermissionAwareButton>
          </>
        )}
      />
      {searchText && <SearchScopeHint loaded={list.items.length} hasNext={list.hasNextPage ?? false} />}
      {selected.length > 0 && (
        <Space style={{ marginBottom: 16 }}>
          <span>{t('status.selected', { n: selected.length })}</span>
          <PermissionAwareButton data-testid={resourceActionTestId(RESOURCE_KIND, 'batch-delete')} danger resourceKind={RESOURCE_KIND} resourceVerb="delete" onClick={() => confirmDelete(selected.map((item) => ({ namespace: item.metadata.namespace!, name: item.metadata.name, resourceVersion: item.metadata.resourceVersion! })), true)}>{t('btn.batchDelete')}</PermissionAwareButton>
        </Space>
      )}
      <Table
        rowKey={(item) => `${item.metadata.namespace}/${item.metadata.name}`}
        columns={columns}
        dataSource={filtered}
        loading={list.isLoading}
        rowSelection={{ selectedRowKeys, onChange: setSelectedRowKeys }}
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !list.hasNextPage,
          showTotal: (count) => list.hasNextPage ? t('table.loadedMore', { n: count }) : t('table.totalItems', { n: count }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= list.items.length && list.hasNextPage && !list.isFetchingNextPage) list.fetchNextPage()
          },
        }}
      />
      <EdgionBackendTrafficPolicyEditor
        visible={editor.visible}
        mode={editor.mode}
        resource={editor.resource}
        onClose={() => setEditor((current) => ({ ...current, visible: false }))}
      />
    </div>
  )
}

export default EdgionBackendTrafficPolicyList
