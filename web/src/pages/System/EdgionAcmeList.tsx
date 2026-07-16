import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Button, Space, Input, Tag, Modal, message, Badge } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined, ThunderboltOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import EdgionAcmeEditor from '@/components/ResourceEditor/EdgionAcme/EdgionAcmeEditor'
import { apiClient } from '@/api/client'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'

const { Search } = Input

const phaseColorMap: Record<string, string> = {
  Ready: 'success', Issuing: 'processing', Renewing: 'processing',
  Pending: 'warning', Failed: 'error',
}

const EdgionAcmeList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [searchText, setSearchText] = useState('')
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  const {
    items: acmeList,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('edgionacme', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name, resourceVersion }: { namespace: string; name: string; resourceVersion: string }) =>
      resourceApi.delete(mutationTarget, 'edgionacme', namespace, name, resourceVersion),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionacme'] })
    },
  })

  const filtered = acmeList.filter((r) => {
    const s = searchText.toLowerCase()
    return r.metadata.name.toLowerCase().includes(s) || r.metadata.namespace?.toLowerCase().includes(s)
  })

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode); setSelectedResource(resource || null); setEditorVisible(true)
  }

  const handleTrigger = async (r: K8sResource) => {
    try {
      await apiClient.post(`/services/acme/${r.metadata.namespace}/${r.metadata.name}/trigger`)
      message.success(t('msg.triggerOk'))
      refetch()
    } catch (e: any) { message.error(t('msg.triggerFailed', { err: e.message })) }
  }

  const columns = [
    ...getResourceMetaColumns<K8sResource>({
      namespaced: true,
      titles: {
        name: t('col.name'),
        namespace: t('col.namespace'),
        age: t('col.age'),
      },
      items: acmeList,
    }),
    { title: t('col.domains'), key: 'domains',
      render: (_: any, r: K8sResource) => (
        <Space wrap>
          {(r.spec?.domains || []).slice(0, 2).map((d: string) => <Tag key={d}>{d}</Tag>)}
          {(r.spec?.domains || []).length > 2 && <Tag>+{(r.spec?.domains || []).length - 2}</Tag>}
        </Space>
      ),
    },
    { title: t('col.challenge'), key: 'challenge',
      render: (_: any, r: K8sResource) => (
        <Tag color="purple">{r.spec?.challenge?.type || '-'}</Tag>
      ),
    },
    { title: 'Lifecycle', key: 'phase',
      render: (_: any, r: K8sResource) => {
        const phase = r.status?.phase
        if (!phase) return '-'
        return <Badge status={phaseColorMap[phase] as any || 'default'} text={phase} />
      },
    },
    { title: t('col.status'), key: 'status', render: (_: unknown, r: K8sResource) => <ResourceConditions status={r.status} compact /> },
    {
      title: t('col.actions'), key: 'actions', width: 200,
      render: (_: any, record: K8sResource) => (
        <Space>
          <Button data-testid={resourceActionTestId('edgionacme', 'row-view')} size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', record)}>{t('btn.view')}</Button>
          <Button data-testid={resourceActionTestId('edgionacme', 'row-edit')} size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', record)}>{t('btn.edit')}</Button>
          <Button data-testid="acme-trigger" size="small" icon={<ThunderboltOutlined />} onClick={() => handleTrigger(record)}>{t('btn.trigger')}</Button>
          <Button data-testid={resourceActionTestId('edgionacme', 'row-delete')} size="small" danger icon={<DeleteOutlined />}
            onClick={() => Modal.confirm({
              ...resourceDeleteConfirmProps,
              title: t('confirm.deleteTitle'), content: t('confirm.deleteMsg', { name: record.metadata.name }),
              okText: t('confirm.okText'), okType: 'danger', cancelText: t('btn.cancel'),
              onOk: () => deleteMutation.mutate({
                namespace: record.metadata.namespace!, name: record.metadata.name, resourceVersion: record.metadata.resourceVersion!,
              }),
            })}>{t('btn.delete')}</Button>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="EdgionAcme"
        subtitle={t('page.subtitle.acme')}
        actions={
          <>
            <Search data-testid={resourceActionTestId('edgionacme', 'search')} placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
              style={{ width: 240 }} allowClear />
            <Button data-testid={resourceActionTestId('edgionacme', 'refresh')} icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>
            <Button data-testid={resourceActionTestId('edgionacme', 'create')} type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</Button>
          </>
        }
      />

      {searchText && (
        <SearchScopeHint loaded={acmeList.length} hasNext={hasNextPage ?? false} />
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
            if (page * pageSize >= acmeList.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />
      <EdgionAcmeEditor visible={editorVisible} mode={editorMode} resource={selectedResource}
        onClose={() => setEditorVisible(false)} />
    </div>
  )
}

export default EdgionAcmeList
