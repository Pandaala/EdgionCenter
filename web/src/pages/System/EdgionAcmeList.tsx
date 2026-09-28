import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Space, Input, Tag, Modal, message } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import EdgionAcmeEditor from '@/components/ResourceEditor/EdgionAcme/EdgionAcmeEditor'
import AcmeLifecycle from '@/components/resource/AcmeLifecycle'
import AcmeTriggerButton from '@/components/resource/AcmeTriggerButton'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import ResourceStatus from '@/components/resource/ResourceStatus'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'

const { Search } = Input

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
    { title: t('acme.lifecycle.title'), key: 'phase', width: 240,
      render: (_: unknown, r: K8sResource) => <AcmeLifecycle resource={r} />,
    },
    { title: t('col.status'), key: 'status', render: (_: unknown, r: K8sResource) => <ResourceStatus kind="edgionacme" resource={r} /> },
    {
      title: t('col.actions'), key: 'actions', width: 200,
      render: (_: any, record: K8sResource) => (
        <Space>
          <PermissionAwareButton resourceKind="edgionacme" resourceVerb="get" data-testid={resourceActionTestId('edgionacme', 'row-view')} size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', record)}>{t('btn.view')}</PermissionAwareButton>
          <PermissionAwareButton resourceKind="edgionacme" resourceVerb="update" data-testid={resourceActionTestId('edgionacme', 'row-edit')} size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', record)}>{t('btn.edit')}</PermissionAwareButton>
          <AcmeTriggerButton resource={record} />
          <PermissionAwareButton resourceKind="edgionacme" resourceVerb="delete" data-testid={resourceActionTestId('edgionacme', 'row-delete')} size="small" danger icon={<DeleteOutlined />}
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
        title="EdgionAcme"
        subtitle={t('page.subtitle.acme')}
        actions={
          <>
            <Search data-testid={resourceActionTestId('edgionacme', 'search')} placeholder={t('ph.searchNameNs')} value={searchText} onChange={(e) => setSearchText(e.target.value)}
              style={{ width: 240 }} allowClear />
            <PermissionAwareButton resourceKind="edgionacme" resourceVerb="list" data-testid={resourceActionTestId('edgionacme', 'refresh')} icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</PermissionAwareButton>
            <PermissionAwareButton resourceKind="edgionacme" resourceVerb="create" data-testid={resourceActionTestId('edgionacme', 'create')} type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</PermissionAwareButton>
          </>
        }
      />

      {searchText && (
        <SearchScopeHint loaded={acmeList.length} hasNext={hasNextPage ?? false} />
      )}

      <Table rowKey={(r) => `${r.metadata.namespace ?? ''}/${r.metadata.name}`}
        columns={columns} dataSource={filtered} loading={isLoading}
        scroll={{ x: 'max-content' }}
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
