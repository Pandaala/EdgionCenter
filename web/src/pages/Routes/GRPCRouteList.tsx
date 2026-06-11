import { useState } from 'react'
import { Table, Button, Space, Input, Tag, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import GRPCRouteEditor from '@/components/ResourceEditor/GRPCRoute/GRPCRouteEditor'
import type { GRPCRoute } from '@/types/gateway-api/grpcroute'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'

const { Search } = Input

const GRPCRouteList = () => {
  const t = useT()
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<GRPCRoute | null>(null)
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
  } = useResourceList<K8sResource>('grpcroute', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name }: { namespace: string; name: string }) =>
      resourceApi.delete('grpcroute', namespace, name),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'grpcroute'] })
    },
  })

  const batchDeleteMutation = useMutation({
    mutationFn: (resources: Array<{ namespace: string; name: string }>) =>
      resourceApi.batchDelete('grpcroute', resources),
    onSuccess: () => {
      message.success(t('msg.batchDeleteOk', { n: selectedRowKeys.length }))
      setSelectedRowKeys([])
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'grpcroute'] })
    },
  })

  const filtered = routes.filter((r) => {
    const s = searchText.toLowerCase()
    return r.metadata.name.toLowerCase().includes(s) || r.metadata.namespace?.toLowerCase().includes(s)
  })

  const handleDelete = (namespace: string, name: string) => {
    Modal.confirm({
      title: t('confirm.deleteTitle'),
      content: t('confirm.deleteMsg', { name }),
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate({ namespace, name }),
    })
  }

  const handleBatchDelete = () => {
    const selected = filtered
      .filter((r) => selectedRowKeys.includes(`${r.metadata.namespace}/${r.metadata.name}`))
      .map((r) => ({ namespace: r.metadata.namespace!, name: r.metadata.name }))
    Modal.confirm({
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
    setSelectedResource(resource as GRPCRoute || null)
    setEditorVisible(true)
  }

  const getRuleCount = (r: K8sResource) => r.spec?.rules?.length || 0

  const getMethodSummary = (r: K8sResource) => {
    const rules = r.spec?.rules || []
    const methods: string[] = []
    rules.forEach((rule: any) => {
      const matches: any[] = rule.matches || []
      matches.forEach((match: any) => {
        if (match.method?.service) {
          const m = match.method.method ? `${match.method.service}/${match.method.method}` : match.method.service
          methods.push(m)
        }
      })
    })
    return methods.slice(0, 2)
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
      title: t('col.grpcMethods'),
      key: 'methods',
      render: (_: any, record: K8sResource) => (
        <Space wrap>
          {getMethodSummary(record).map((m) => <Tag key={m} color="geekblue">{m}</Tag>)}
          {getRuleCount(record) > 0 && (
            <Tag>{t('grpc.ruleCount', { n: getRuleCount(record) })}</Tag>
          )}
        </Space>
      ),
    },
    {
      title: t('col.actions'),
      key: 'actions',
      width: 160,
      render: (_: any, record: K8sResource) => (
        <Space>
          <Button size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', record)}>{t('btn.view')}</Button>
          <Button size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', record)}>{t('btn.edit')}</Button>
          <Button size="small" danger icon={<DeleteOutlined />}
            onClick={() => handleDelete(record.metadata.namespace!, record.metadata.name)}>{t('btn.delete')}</Button>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="GRPCRoute"
        subtitle={t('page.subtitle.grpcRoute')}
        actions={
          <>
            <Button icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</Button>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
          style={{ width: 240 }} allowClear />
      </div>

      {searchText && (
        <SearchScopeHint loaded={routes.length} hasNext={hasNextPage ?? false} />
      )}

      {selectedRowKeys.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <Space>
            <span>{t('status.selected', { n: selectedRowKeys.length })}</span>
            <Button danger onClick={handleBatchDelete}>{t('btn.batchDelete')}</Button>
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

      <GRPCRouteEditor
        visible={editorVisible}
        mode={editorMode}
        resource={selectedResource}
        onClose={() => setEditorVisible(false)}
      />
    </div>
  )
}

export default GRPCRouteList
