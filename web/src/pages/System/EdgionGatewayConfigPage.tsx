/**
 * EdgionGatewayConfig 管理页面（集群级，通常单例）
 */

import { useState } from 'react'
import { Table, Button, Space, Modal, message, Tag } from 'antd'
import { PlusOutlined, ReloadOutlined, EyeOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useParams } from 'react-router-dom'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { clusterResourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import EdgionGatewayConfigEditor from '@/components/ResourceEditor/EdgionGatewayConfig/EdgionGatewayConfigEditor'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'

const EdgionGatewayConfigPage = () => {
  const t = useT()
  const [editorVisible, setEditorVisible] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit' | 'view'>('create')
  const [selectedResource, setSelectedResource] = useState<K8sResource | null>(null)
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  const { data, isLoading, refetch } = useQuery({
    queryKey: ['edgiongatewayconfig', controllerId ?? ''],
    queryFn: () => clusterResourceApi.listAll<K8sResource>('edgiongatewayconfig'),
  })

  const deleteMutation = useMutation({
    mutationFn: (name: string) => clusterResourceApi.delete('edgiongatewayconfig', name),
    onSuccess: () => { message.success(t('msg.deleteOk')); queryClient.invalidateQueries({ queryKey: ['edgiongatewayconfig', controllerId ?? ''] }) },
  })

  const items = data?.data || []

  const openEditor = (mode: 'create' | 'edit' | 'view', resource?: K8sResource) => {
    setEditorMode(mode); setSelectedResource(resource || null); setEditorVisible(true)
  }

  const columns = [
    { title: t('col.name'), dataIndex: ['metadata', 'name'], key: 'name' },
    { title: 'MaxRetries', key: 'retries',
      render: (_: any, r: K8sResource) => r.spec?.maxRetries ?? '-' },
    { title: 'Preflight Mode', key: 'preflight',
      render: (_: any, r: K8sResource) => r.spec?.preflightPolicy?.mode
        ? <Tag>{r.spec.preflightPolicy.mode}</Tag> : '-' },
    { title: 'Real IP Header', key: 'realip',
      render: (_: any, r: K8sResource) => r.spec?.realIp?.realIpHeader || '-' },
    {
      title: t('col.actions'), key: 'actions', width: 160,
      render: (_: any, r: K8sResource) => (
        <Space>
          <Button size="small" icon={<EyeOutlined />} onClick={() => openEditor('view', r)}>{t('btn.view')}</Button>
          <Button size="small" icon={<EditOutlined />} onClick={() => openEditor('edit', r)}>{t('btn.edit')}</Button>
          <Button size="small" danger icon={<DeleteOutlined />}
            onClick={() => Modal.confirm({
              title: t('confirm.deleteTitle'), content: t('confirm.deleteMsg', { name: r.metadata.name }),
              okText: t('confirm.okText'), okType: 'danger', cancelText: t('btn.cancel'),
              onOk: () => deleteMutation.mutate(r.metadata.name),
            })}>{t('btn.delete')}</Button>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <PageHeader
        title="EdgionGatewayConfig"
        subtitle={t('page.subtitle.gatewayConfig')}
        actions={
          <>
            <Button icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>{t('btn.create')}</Button>
          </>
        }
      />
      <Table rowKey={(r) => r.metadata.name} columns={columns} dataSource={items}
        loading={isLoading} pagination={{ pageSize: 20 }} size="middle" />
      <EdgionGatewayConfigEditor visible={editorVisible} mode={editorMode} resource={selectedResource}
        onClose={() => setEditorVisible(false)} />
    </div>
  )
}

export default EdgionGatewayConfigPage
