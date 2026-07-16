/**
 * Gateway 编辑器 Modal
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import GatewayForm from './GatewayForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { Gateway } from '@/types/gateway-api/gateway'
import {
  createEmptyGateway,
  normalizeGateway,
  gatewayToMutationYaml,
  gatewayToYaml,
  yamlToGateway,
  validateGateway,
} from '@/utils/gateway'
import { useT } from '@/i18n'
import GatewayStatusDetails from './GatewayStatusDetails'
import { useEditorTabTransition } from '../useEditorTabTransition'

interface GatewayEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: Gateway | null
  onClose: () => void
}

const GatewayEditor: React.FC<GatewayEditorProps> = ({ visible, mode, resource, onClose }) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState<Gateway>(() => createEmptyGateway())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()
  const { activeTab, editableTab, resetEditorTab, handleTabChange } = useEditorTabTransition({
    formData, yamlContent, serialize: gatewayToYaml, parse: yamlToGateway, setFormData, setYamlContent,
    onError: (error) => message.error(t('msg.tabSwitchFailed', { err: error.message })),
  })

  useEffect(() => {
    if (!visible) return
    resetEditorTab()
    if (mode === 'create') {
      const empty = createEmptyGateway()
      setFormData(empty)
      setYamlContent(gatewayToYaml(empty))
    } else if (resource) {
      const normalized = normalizeGateway(resource)
      setFormData(normalized)
      setYamlContent(gatewayToYaml(normalized))
    }
  }, [visible, mode, resource, resetEditorTab])

  const createMutation = useMutation({
    mutationFn: ({ namespace, yamlStr }: { namespace: string; yamlStr: string }) =>
      resourceApi.create(mutationTarget, 'gateway', namespace, yamlStr),
    onSuccess: () => { message.success(t('msg.createOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'gateway'] }); onClose() },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, yamlStr }: { namespace: string; name: string; yamlStr: string }) =>
      resourceApi.update(mutationTarget, 'gateway', namespace, name, yamlStr),
    onSuccess: () => { message.success(t('msg.updateOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'gateway'] }); onClose() },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const sourceYaml = editableTab === 'yaml' ? yamlContent : gatewayToYaml(formData)
      const parsed = yamlToGateway(sourceYaml)
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
      const validationErrors = validateGateway(parsed)
      if (validationErrors.length > 0) {
        message.error(t('msg.submitFailed', { err: validationErrors.join('; ') }))
        return
      }
      const yamlStr = gatewayToMutationYaml(parsed, mode === 'create' ? 'create' : 'update')
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
      ? t('modal.create', { resource: 'Gateway' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'Gateway' })
      : t('modal.view', { resource: 'Gateway' })

  return (
    <Modal
      title={title}
      open={visible} onCancel={onClose} width={920}
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
          children: <GatewayForm data={formData} onChange={setFormData} readOnly={isReadOnly} isCreate={mode === 'create'} />,
        },
        {
          key: 'yaml', label: editorYamlTab(t('tab.yaml')),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={isReadOnly} height="500px" />,
        },
        ...(mode !== 'create' ? [{
          key: 'conditions', label: t('tab.conditions'),
          children: <GatewayStatusDetails status={formData.status} />,
        }] : []),
      ]} />
    </Modal>
  )
}

export default GatewayEditor
