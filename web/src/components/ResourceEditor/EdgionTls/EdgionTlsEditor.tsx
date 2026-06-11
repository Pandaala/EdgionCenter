/**
 * EdgionTls 编辑器 Modal
 */

import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import EdgionTlsForm from './EdgionTlsForm'
import type { EdgionTls } from '@/types/edgion-tls'
import { createEmptyEdgionTls, normalizeEdgionTls, edgionTlsToYaml, yamlToEdgionTls } from '@/utils/edgiontls'
import { useT } from '@/i18n'

interface EdgionTlsEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: EdgionTls | null
  onClose: () => void
}

const EdgionTlsEditor: React.FC<EdgionTlsEditorProps> = ({ visible, mode, resource, onClose }) => {
  const t = useT()
  const [activeTab, setActiveTab] = useState<'form' | 'yaml'>('form')
  const [formData, setFormData] = useState<EdgionTls>(() => createEmptyEdgionTls())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()

  useEffect(() => {
    if (!visible) return
    setActiveTab('form')
    if (mode === 'create') {
      const empty = createEmptyEdgionTls()
      setFormData(empty)
      setYamlContent(edgionTlsToYaml(empty))
    } else if (resource) {
      const normalized = normalizeEdgionTls(resource)
      setFormData(normalized)
      setYamlContent(edgionTlsToYaml(normalized))
    }
  }, [visible, mode, resource])

  const handleTabChange = (key: string) => {
    try {
      if (key === 'yaml') setYamlContent(edgionTlsToYaml(formData))
      else setFormData(yamlToEdgionTls(yamlContent))
      setActiveTab(key as 'form' | 'yaml')
    } catch (e: any) { message.error(t('msg.tabSwitchFailed', { err: e.message })) }
  }

  const createMutation = useMutation({
    mutationFn: ({ namespace, yamlStr }: { namespace: string; yamlStr: string }) =>
      resourceApi.create('edgiontls', namespace, yamlStr),
    onSuccess: () => { message.success(t('msg.createOk')); queryClient.invalidateQueries({ queryKey: ['edgiontls'] }); onClose() },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, yamlStr }: { namespace: string; name: string; yamlStr: string }) =>
      resourceApi.update('edgiontls', namespace, name, yamlStr),
    onSuccess: () => { message.success(t('msg.updateOk')); queryClient.invalidateQueries({ queryKey: ['edgiontls'] }); onClose() },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const yamlStr = activeTab === 'yaml' ? yamlContent : edgionTlsToYaml(formData)
      const parsed = yamlToEdgionTls(yamlStr)
      const name = parsed.metadata?.name
      const namespace = parsed.metadata?.namespace
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
      ? t('modal.create', { resource: 'EdgionTls' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'EdgionTls' })
      : t('modal.view', { resource: 'EdgionTls' })

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
        { key: 'form', label: t('tab.form'),
          children: <EdgionTlsForm data={formData} onChange={setFormData} readOnly={isReadOnly} isCreate={mode === 'create'} /> },
        { key: 'yaml', label: t('tab.yaml'),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={isReadOnly} height="500px" /> },
      ]} />
    </Modal>
  )
}

export default EdgionTlsEditor
