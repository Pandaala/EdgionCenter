import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useState } from 'react'
import { Table, Space, Input, Tag, Modal, message, Badge, Tooltip } from 'antd'
import {
  PlusOutlined,
  ReloadOutlined,
  EyeOutlined,
  EditOutlined,
  DeleteOutlined,
} from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { batchDeleteFailureKeys, resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import EdgionPluginsEditor from '@/components/ResourceEditor/EdgionPlugins/EdgionPluginsEditor'
import { countPluginsByStage } from '@/utils/edgionplugins'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useResourceList } from '@/hooks/useResourceList'
import { getResourceMetaColumns } from '@/components/resource/resourceMetaColumns'
import SearchScopeHint from '@/components/resource/SearchScopeHint'
import ResourceListError from '@/components/resource/ResourceListError'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { resourceActionTestId } from '@/components/resource/testIds'
import { resourceBatchDeleteConfirmProps, resourceDeleteConfirmProps } from '@/components/resource/confirmTestIds'
import ResourceConditions from '@/components/resource/ResourceConditions'

const { Search } = Input

const EdgionPluginsList = () => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  const {
    items: plugins,
    isLoading,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useResourceList<K8sResource>('edgionplugins', {
    namespaced: true,
    scope: controllerId ?? null,
  })

  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name, resourceVersion }: { namespace: string; name: string; resourceVersion: string }) =>
      resourceApi.delete(mutationTarget, 'edgionplugins', namespace, name, resourceVersion),
    onSuccess: () => {
      message.success(t('msg.deleteOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionplugins'] })
    },
  })

  const batchDeleteMutation = useMutation({
    mutationFn: (resources: Array<{ namespace: string; name: string; resourceVersion: string }>) =>
      resourceApi.batchDelete(mutationTarget, 'edgionplugins', resources),
    onSuccess: () => {
      message.success(t('msg.batchDeleteOk', { n: selectedRowKeys.length }))
      setSelectedRowKeys([])
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionplugins'] })
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

  const filteredList = plugins.filter((p) => {
    const lower = searchText.toLowerCase()
    return (
      p.metadata.name.toLowerCase().includes(lower) ||
      p.metadata.namespace?.toLowerCase().includes(lower)
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
    const selectedResources = filteredList
      .filter((p) => selectedRowKeys.includes(`${p.metadata.namespace}/${p.metadata.name}`))
      .map((p) => ({ namespace: p.metadata.namespace!, name: p.metadata.name, resourceVersion: p.metadata.resourceVersion! }))

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
    setSelectedResource(record)
    setEditorVisible(true)
  }

  const handleEdit = (record: K8sResource) => {
    setEditorMode('edit')
    setSelectedResource(record)
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
      items: plugins,
    }),
    {
      title: t('col.totalPlugins'),
      key: 'pluginCount',
      render: (_: any, record: K8sResource) => {
        const stages = countPluginsByStage((record as any).spec)
        const total = stages.request + stages.responseFilter + stages.responseBodyFilter + stages.response
        const tooltipText = [
          `${t('plugins.requestStage')}: ${stages.request}`,
          `${t('plugins.responseFilter')}: ${stages.responseFilter}`,
          `${t('plugins.responseBody')}: ${stages.responseBodyFilter}`,
          `${t('plugins.upstreamResponse')}: ${stages.response}`,
        ].join(' | ')
        return (
          <Tooltip title={tooltipText}>
            <Badge
              count={total}
              showZero
              style={{ backgroundColor: total > 0 ? '#1677ff' : 'var(--ec-color-border)' }}
            />
          </Tooltip>
        )
      },
    },
    {
      title: t('col.stageDistrib'),
      key: 'stageSummary',
      render: (_: any, record: K8sResource) => {
        const stages = countPluginsByStage((record as any).spec)
        const total = stages.request + stages.responseFilter + stages.responseBodyFilter + stages.response
        return (
          <Space size={4}>
            {stages.request > 0 && (
              <Tooltip title={`${t('plugins.requestStage')}: ${t('plugins.stagePlugins', { n: stages.request })}`}>
                <Tag color="blue">Req×{stages.request}</Tag>
              </Tooltip>
            )}
            {stages.responseFilter > 0 && (
              <Tooltip title={`${t('plugins.responseFilter')}: ${t('plugins.stagePlugins', { n: stages.responseFilter })}`}>
                <Tag color="green">RespFilter×{stages.responseFilter}</Tag>
              </Tooltip>
            )}
            {stages.responseBodyFilter > 0 && (
              <Tooltip title={`${t('plugins.responseBody')}: ${t('plugins.stagePlugins', { n: stages.responseBodyFilter })}`}>
                <Tag color="orange">BodyFilter×{stages.responseBodyFilter}</Tag>
              </Tooltip>
            )}
            {stages.response > 0 && (
              <Tooltip title={`${t('plugins.upstreamResponse')}: ${t('plugins.stagePlugins', { n: stages.response })}`}>
                <Tag color="purple">Resp×{stages.response}</Tag>
              </Tooltip>
            )}
            {total === 0 && <Tag color="default">{t('plugins.noPlugins')}</Tag>}
          </Space>
        )
      },
    },
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
          <PermissionAwareButton resourceKind="edgionplugins" resourceVerb="get" type="link" icon={<EyeOutlined />} size="small" onClick={() => handleView(record)}>
            {t('btn.view')}
          </PermissionAwareButton>
          <PermissionAwareButton resourceKind="edgionplugins" resourceVerb="update" type="link" icon={<EditOutlined />} size="small" onClick={() => handleEdit(record)}>
            {t('btn.edit')}
          </PermissionAwareButton>
          <PermissionAwareButton
            resourceKind="edgionplugins" resourceVerb="delete"
            type="link"
            danger
            icon={<DeleteOutlined />}
            size="small"
            onClick={() => handleDelete(record.metadata.namespace!, record.metadata.name, record.metadata.resourceVersion!)}
          >
            {t('btn.delete')}
          </PermissionAwareButton>
        </Space>
      ),
    },
  ]

  if (error) return <ResourceListError error={error} onRetry={refetch} />

  return (
    <div>
      <PageHeader
        title="EdgionPlugins"
        subtitle={t('page.subtitle.plugins')}
        actions={
          <>
            <PermissionAwareButton resourceKind="edgionplugins" resourceVerb="list" icon={<ReloadOutlined />} onClick={() => refetch()}>
              {t('btn.refresh')}
            </PermissionAwareButton>
            <PermissionAwareButton resourceKind="edgionplugins" resourceVerb="create" type="primary" icon={<PlusOutlined />} onClick={handleCreate}>
              {t('btn.create')}
            </PermissionAwareButton>
          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <Search
          data-testid={resourceActionTestId('edgionplugins', 'search')}
          placeholder={t('ph.searchNameNs')}
          allowClear
          style={{ width: 300 }}
          value={searchText}
          onChange={(e) => setSearchText(e.target.value)}
        />
      </div>

      {searchText && (
        <SearchScopeHint loaded={plugins.length} hasNext={hasNextPage ?? false} />
      )}

      {selectedRowKeys.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <Space>
            <span>{t('status.selected', { n: selectedRowKeys.length })}</span>
            <PermissionAwareButton data-testid={resourceActionTestId('edgionplugins', 'batch-delete')} resourceKind="edgionplugins" resourceVerb="delete" danger onClick={handleBatchDelete}>
              {t('btn.batchDelete')}
            </PermissionAwareButton>
          </Space>
        </div>
      )}

      <Table
        rowSelection={{
          selectedRowKeys,
          onChange: setSelectedRowKeys,
        }}
        columns={columns}
        dataSource={filteredList}
        loading={isLoading}
        rowKey={(record) => `${record.metadata.namespace ?? ''}/${record.metadata.name}`}
        pagination={{
          defaultPageSize: 20,
          showSizeChanger: true,
          showQuickJumper: !hasNextPage,
          showTotal: (n) =>
            hasNextPage ? t('table.loadedMore', { n }) : t('table.totalItems', { n }),
          onChange: (page, pageSize) => {
            if (page * pageSize >= plugins.length && hasNextPage && !isFetchingNextPage) {
              fetchNextPage()
            }
          },
        }}
      />

      <EdgionPluginsEditor
        visible={editorVisible}
        mode={editorMode}
        resource={selectedResource}
        onClose={handleEditorClose}
      />
    </div>
  )
}

export default EdgionPluginsList
