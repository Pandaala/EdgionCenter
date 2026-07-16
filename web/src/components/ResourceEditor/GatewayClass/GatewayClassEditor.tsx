/**
 * GatewayClass 编辑器 Modal
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { clusterResourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import GatewayClassForm from './GatewayClassForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { GatewayClass } from '@/utils/gatewayclass'
import { createEmpty, normalize, toYaml, fromYaml, toMutationDocument, validateGatewayClass } from '@/utils/gatewayclass'
import { dumpYaml } from '@/utils/yaml-utils'
import { useT } from '@/i18n'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { useEditorTabTransition } from '../useEditorTabTransition'

interface GatewayClassEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: any | null
  onClose: () => void
}

const GatewayClassEditor: React.FC<GatewayClassEditorProps> = ({ visible, mode, resource, onClose }) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState<GatewayClass>(() => createEmpty())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()
  const { activeTab, editableTab, resetEditorTab, handleTabChange } = useEditorTabTransition({
    formData, yamlContent, serialize: toYaml, parse: fromYaml, setFormData, setYamlContent,
    onError: (error) => message.error(t('msg.tabSwitchFailed', { err: error.message })),
  })

  useEffect(() => {
    if (!visible) return
    resetEditorTab()
    if (mode === 'create') {
      const empty = createEmpty()
      setFormData(empty)
      setYamlContent(toYaml(empty))
    } else if (resource) {
      const normalized = normalize(resource)
      setFormData(normalized)
      setYamlContent(toYaml(normalized))
    }
  }, [visible, mode, resource, resetEditorTab])

  const createMutation = useMutation({
    mutationFn: ({ yamlStr }: { yamlStr: string }) =>
      clusterResourceApi.create(mutationTarget, 'gatewayclass', yamlStr),
    onSuccess: () => { message.success(t('msg.createOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'gatewayclass'] }); onClose() },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ name, yamlStr }: { name: string; yamlStr: string }) =>
      clusterResourceApi.update(mutationTarget, 'gatewayclass', name, yamlStr),
    onSuccess: () => { message.success(t('msg.updateOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'gatewayclass'] }); onClose() },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const parsed = editableTab === 'form' ? formData : fromYaml(yamlContent)
      const name = parsed.metadata?.name
      const yamlStr = dumpYaml(toMutationDocument(parsed, mode === 'create' ? 'create' : 'update'))
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
      const validationErrors = validateGatewayClass(parsed)
      if (validationErrors.length > 0) {
        message.error(t('msg.submitFailed', { err: validationErrors.join('; ') }))
        return
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
          children: <GatewayClassForm data={formData} onChange={setFormData} readOnly={isReadOnly} isCreate={mode === 'create'} />,
        },
        {
          key: 'yaml', label: editorYamlTab(t('tab.yaml')),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={isReadOnly} height="500px" />,
        },
        ...(mode !== 'create' ? [{
          key: 'conditions', label: t('tab.conditions'),
          children: <ResourceConditions status={formData.status} emptyText={t('status.noConditions')} />,
        }] : []),
      ]} />
    </Modal>
  )
}

export default GatewayClassEditor
