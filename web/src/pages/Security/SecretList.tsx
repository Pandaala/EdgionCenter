import { useState } from 'react'
import { Table, Button, Space, Input, Tag, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import SecretEditor from '@/components/ResourceEditor/Secret/SecretEditor'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'

const { Search } = Input

const SecretList = () => {
  const t = useT()
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  const {
    items: secrets,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('secret', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name }: { namespace: string; name: string }) =>
      resourceApi.delete('secret', namespace, name),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'secret'] })
    },
  })

  const batchDeleteMutation = useMutation({
    mutationFn: (resources: Array<{ namespace: string; name: string }>) =>
      resourceApi.batchDelete('secret', resources),
    onSuccess: () => {
      message.success(t('msg.batchDeleteOk', { n: selectedRowKeys.length }))
      setSelectedRowKeys([])
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'secret'] })
    },
  })

  const filtered = secrets.filter((r) => {
    const s = searchText.toLowerCase()
    return r.metadata.name.toLowerCase().includes(s) || r.metadata.namespace?.toLowerCase().includes(s)
  })

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode); setSelectedResource(resource || null); setEditorVisible(true)
  }

  const handleDelete = (namespace: string, name: string) => {
    Modal.confirm({
      title: t('confirm.deleteTitle'), content: t('confirm.deleteMsg', { name: `Secret ${name}` }),
      okText: t('confirm.okText'), okType: 'danger', cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate({ namespace, name }),
    })
  }

  const handleBatchDelete = () => {
    const selected = filtered
      .filter((r) => selectedRowKeys.includes(`${r.metadata.namespace}/${r.metadata.name}`))
      .map((r) => ({ namespace: r.metadata.namespace!, name: r.metadata.name }))
    Modal.confirm({
      title: t('confirm.batchDeleteTitle'), content: `${t('confirm.batchDeleteMsg', { n: selected.length })} ${t('confirm.deleteIrreversible')}`,
      okText: t('confirm.okText'), okType: 'danger', cancelText: t('btn.cancel'),
      onOk: () => batchDeleteMutation.mutate(selected),
    })
  }

  const getDataKeys = (r: K8sResource) => {
    const d = (r as any)['data'] || {}
    return Object.keys(d).join(', ')
  }

  const columns = [
    ...getResourceMetaColumns<K8sResource>({
      namespaced: true,
      titles: {
        name: t('col.name'),
        namespace: t('col.namespace'),
        age: t('col.age'),
      },
      items: secrets,
    }),
    { title: t('col.type'), key: 'type',
      render: (_: any, r: K8sResource) => {
        const secretType = (r as any)['type'] || 'Opaque'
        const color = secretType === 'kubernetes.io/tls' ? 'green' : secretType === 'Opaque' ? 'blue' : 'default'
        return <Tag color={color}>{secretType}</Tag>
      },
    },
    { title: t('col.keys'), key: 'keys',
      render: (_: any, r: K8sResource) => <span style={{ color: 'var(--ec-color-text-muted)', fontSize: 12 }}>{getDataKeys(r)}</span> },
    {
      title: t('col.actions'), key: 'actions', width: 160,
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
      <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <Space>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</Button>
          <Button danger disabled={!selectedRowKeys.length} icon={<DeleteOutlined />} onClick={handleBatchDelete}>
            {`${t('btn.batchDelete')}${selectedRowKeys.length > 0 ? ` (${selectedRowKeys.length})` : ''}`}
          </Button>
        </Space>
        <Space>
          <Search placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
            style={{ width: 240 }} allowClear />
          <Button icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>
        </Space>
      </div>

      {searchText && (
        <SearchScopeHint loaded={secrets.length} hasNext={hasNextPage ?? false} />
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
            if (page * pageSize >= secrets.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />
      <SecretEditor visible={editorVisible} mode={editorMode} resource={selectedResource}
        onClose={() => setEditorVisible(false)} />
    </div>
  )
}

export default SecretList
