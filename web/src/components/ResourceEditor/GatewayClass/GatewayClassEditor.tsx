/**
 * GatewayClass 编辑器 Modal
 */

import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { clusterResourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import GatewayClassForm from './GatewayClassForm'
import type { GatewayClass } from '@/utils/gatewayclass'
import { createEmpty, normalize, toYaml, fromYaml } from '@/utils/gatewayclass'
import { useT } from '@/i18n'

interface GatewayClassEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: any | null
  onClose: () => void
}

const GatewayClassEditor: React.FC<GatewayClassEditorProps> = ({ visible, mode, resource, onClose }) => {
  const t = useT()
  const [activeTab, setActiveTab] = useState<'form' | 'yaml'>('form')
  const [formData, setFormData] = useState<GatewayClass>(() => createEmpty())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()

  useEffect(() => {
    if (!visible) return
    setActiveTab('form')
    if (mode === 'create') {
      const empty = createEmpty()
      setFormData(empty)
      setYamlContent(toYaml(empty))
    } else if (resource) {
      const normalized = normalize(resource)
      setFormData(normalized)
      setYamlContent(toYaml(normalized))
    }
  }, [visible, mode, resource])

  const handleTabChange = (key: string) => {
    try {
      if (key === 'yaml') setYamlContent(toYaml(formData))
      else setFormData(fromYaml(yamlContent))
      setActiveTab(key as 'form' | 'yaml')
    } catch (e: any) { message.error(t('msg.tabSwitchFailed', { err: e.message })) }
  }

  const createMutation = useMutation({
    mutationFn: ({ yamlStr }: { yamlStr: string }) =>
      clusterResourceApi.create('gatewayclass', yamlStr),
    onSuccess: () => { message.success(t('msg.createOk')); queryClient.invalidateQueries({ queryKey: ['gatewayclass'] }); onClose() },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ name, yamlStr }: { name: string; yamlStr: string }) =>
      clusterResourceApi.update('gatewayclass', name, yamlStr),
    onSuccess: () => { message.success(t('msg.updateOk')); queryClient.invalidateQueries({ queryKey: ['gatewayclass'] }); onClose() },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const isFormTab = activeTab === 'form'
      const name = isFormTab ? formData.metadata?.name : fromYaml(yamlContent).metadata?.name
      const yamlStr = isFormTab ? toYaml(formData) : yamlContent
      if (!name) {
        message.error(t('msg.metaNameRequired'))
        return
      }
      if (mode !== 'create' && resource) {
        if (name !== resource.metadata.name) {
          message.error(t('msg.noRename'))
          return
        }
      }
      if (mode === 'create') createMutation.mutate({ yamlStr })
      else updateMutation.mutate({ name, yamlStr })
    } catch (e: any) {
      message.error(t('msg.submitFailed', { err: e.message || 'unknown error' }))
    }
  }

  const isPending = createMutation.isPending || updateMutation.isPending
  const isReadOnly = mode === 'view'

  const title =
    mode === 'create'
      ? t('modal.create', { resource: 'GatewayClass' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'GatewayClass' })
      : t('modal.view', { resource: 'GatewayClass' })

  return (
    <Modal
      title={title}
      open={visible} onCancel={onClose} width={860}
      destroyOnClose
      style={{ top: 20 }}
      footer={
        isReadOnly
          ? [<Button key="close" onClick={onClose}>{t('btn.close')}</Button>]
          : [
              <Button key="cancel" onClick={onClose}>{t('btn.cancel')}</Button>,
              <Button key="submit" type="primary" onClick={handleSubmit} loading={isPending}>
                {mode === 'create' ? t('btn.create') : t('btn.save')}
              </Button>,
            ]
      }
    >
      <Tabs activeKey={activeTab} onChange={handleTabChange} items={[
        {
          key: 'form', label: t('tab.form'),
          children: <GatewayClassForm data={formData} onChange={setFormData} readOnly={isReadOnly} isCreate={mode === 'create'} />,
        },
        {
          key: 'yaml', label: t('tab.yaml'),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={isReadOnly} height="500px" />,
        },
      ]} />
    </Modal>
  )
}

export default GatewayClassEditor
