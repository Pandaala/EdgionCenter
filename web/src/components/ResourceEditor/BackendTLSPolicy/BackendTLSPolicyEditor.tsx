/**
 * BackendTLSPolicy 编辑器 Modal
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import BackendTLSPolicyForm from './BackendTLSPolicyForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { BackendTLSPolicy } from '@/utils/backendtlspolicy'
import { createEmpty, normalize, toMutationYaml, toYaml, fromYaml } from '@/utils/backendtlspolicy'
import { useT } from '@/i18n'
import ResourceConditions from '@/components/resource/ResourceConditions'

interface BackendTLSPolicyEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: any | null
  onClose: () => void
}

const BackendTLSPolicyEditor: React.FC<BackendTLSPolicyEditorProps> = ({ visible, mode, resource, onClose }) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [activeTab, setActiveTab] = useState<'form' | 'yaml' | 'conditions'>('form')
  const [formData, setFormData] = useState<BackendTLSPolicy>(() => createEmpty())
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
      else if (activeTab === 'yaml') setFormData(fromYaml(yamlContent))
      setActiveTab(key as 'form' | 'yaml' | 'conditions')
    } catch (e: any) { message.error(t('msg.tabSwitchFailed', { err: e.message })) }
  }

  const createMutation = useMutation({
    mutationFn: ({ namespace, yamlStr }: { namespace: string; yamlStr: string }) =>
      resourceApi.create(mutationTarget, 'backendtlspolicy', namespace, yamlStr),
    onSuccess: () => { message.success(t('msg.createOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'backendtlspolicy'] }); onClose() },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, yamlStr }: { namespace: string; name: string; yamlStr: string }) =>
      resourceApi.update(mutationTarget, 'backendtlspolicy', namespace, name, yamlStr),
    onSuccess: () => { message.success(t('msg.updateOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'backendtlspolicy'] }); onClose() },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const source = activeTab === 'yaml' ? fromYaml(yamlContent) : formData
      const name = source.metadata?.name
      const namespace = source.metadata?.namespace
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
      const yamlStr = toMutationYaml(source, mode === 'create' ? 'create' : 'update')
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
      ? t('modal.create', { resource: 'BackendTLSPolicy' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'BackendTLSPolicy' })
      : t('modal.view', { resource: 'BackendTLSPolicy' })

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
              <Button {...editorCancelButtonProps} key="cancel" onClick={onClose}>{t('btn.cancel')}</Button>,
              <Button {...editorSubmitButtonProps} key="submit" type="primary" onClick={handleSubmit} loading={isPending}>
                {mode === 'create' ? t('btn.create') : t('btn.save')}
              </Button>,
            ]
      }
    >
      <Tabs activeKey={activeTab} onChange={handleTabChange} items={[
        {
          key: 'form', label: editorFormTab(t('tab.form')),
          children: <BackendTLSPolicyForm data={formData} onChange={setFormData} readOnly={isReadOnly} isCreate={mode === 'create'} />,
        },
        {
          key: 'yaml', label: editorYamlTab(t('tab.yaml')),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={isReadOnly} height="500px" />,
        },
        {
          key: 'conditions', label: 'Conditions', disabled: mode === 'create',
          children: <ResourceConditions status={resource?.status ?? formData.status} />,
        },
      ]} />
    </Modal>
  )
}

export default BackendTLSPolicyEditor
