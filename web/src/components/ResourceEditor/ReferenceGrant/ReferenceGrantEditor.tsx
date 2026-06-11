/**
 * ReferenceGrant 编辑器 Modal
 */

import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import ReferenceGrantForm from './ReferenceGrantForm'
import type { ReferenceGrant } from '@/utils/referencegrant'
import { createEmpty, normalize, toYaml, fromYaml } from '@/utils/referencegrant'
import { useT } from '@/i18n'

interface ReferenceGrantEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: any | null
  onClose: () => void
}

const ReferenceGrantEditor: React.FC<ReferenceGrantEditorProps> = ({ visible, mode, resource, onClose }) => {
  const t = useT()
  const [activeTab, setActiveTab] = useState<'form' | 'yaml'>('form')
  const [formData, setFormData] = useState<ReferenceGrant>(() => createEmpty())
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
    mutationFn: ({ namespace, yamlStr }: { namespace: string; yamlStr: string }) =>
      resourceApi.create('referencegrant', namespace, yamlStr),
    onSuccess: () => { message.success(t('msg.createOk')); queryClient.invalidateQueries({ queryKey: ['referencegrant'] }); onClose() },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, yamlStr }: { namespace: string; name: string; yamlStr: string }) =>
      resourceApi.update('referencegrant', namespace, name, yamlStr),
    onSuccess: () => { message.success(t('msg.updateOk')); queryClient.invalidateQueries({ queryKey: ['referencegrant'] }); onClose() },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const isFormTab = activeTab === 'form'
      const name = isFormTab ? formData.metadata?.name : fromYaml(yamlContent).metadata?.name
      const namespace = isFormTab ? formData.metadata?.namespace : fromYaml(yamlContent).metadata?.namespace
      const yamlStr = isFormTab ? toYaml(formData) : yamlContent
      if (!name || !namespace) {
        message.error(t('msg.metaRequired'))
        return
      }
      if (mode !== 'create' && resource) {
        if (name !== resource.metadata.name || namespace !== resource.metadata.namespace) {
          message.error(t('msg.noRename'))
          return
        }
      }
      if (mode === 'create') createMutation.mutate({ namespace, yamlStr })
      else updateMutation.mutate({ namespace, name, yamlStr })
    } catch (e: any) {
      message.error(t('msg.submitFailed', { err: e.message || 'unknown error' }))
    }
  }

  const isPending = createMutation.isPending || updateMutation.isPending
  const isReadOnly = mode === 'view'

  const title =
    mode === 'create'
      ? t('modal.create', { resource: 'ReferenceGrant' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'ReferenceGrant' })
      : t('modal.view', { resource: 'ReferenceGrant' })

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
          children: <ReferenceGrantForm data={formData} onChange={setFormData} readOnly={isReadOnly} isCreate={mode === 'create'} />,
        },
        {
          key: 'yaml', label: t('tab.yaml'),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={isReadOnly} height="500px" />,
        },
      ]} />
    </Modal>
  )
}

export default ReferenceGrantEditor
