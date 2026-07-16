/**
 * EdgionGatewayConfig 编辑器 Modal（集群级资源）
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { clusterResourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import EdgionGatewayConfigForm from './EdgionGatewayConfigForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { EdgionGatewayConfig } from '@/types/edgion-gateway-config'
import { createEmpty, normalize, toMutationYaml, toYaml, fromYaml, validateEdgionGatewayConfig } from '@/utils/edgiongatewayconfig'
import { useT } from '@/i18n'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { useEditorTabTransition } from '../useEditorTabTransition'

interface EdgionGatewayConfigEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: any | null
  onClose: () => void
}

const EdgionGatewayConfigEditor: React.FC<EdgionGatewayConfigEditorProps> = ({
  visible,
  mode,
  resource,
  onClose,
}) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState<EdgionGatewayConfig>(() => createEmpty())
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
    mutationFn: ({ yamlStr }: { yamlStr: string }) => clusterResourceApi.create(mutationTarget, 'edgiongatewayconfig', yamlStr),
    onSuccess: () => {
      message.success(t('msg.createOk'))
      queryClient.invalidateQueries({ queryKey: ['edgiongatewayconfig'] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ name, yamlStr }: { name: string; yamlStr: string }) =>
      clusterResourceApi.update(mutationTarget, 'edgiongatewayconfig', name, yamlStr),
    onSuccess: () => {
      message.success(t('msg.updateOk'))
      queryClient.invalidateQueries({ queryKey: ['edgiongatewayconfig'] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const source = editableTab === 'form' ? formData : fromYaml(yamlContent)
      const name = source.metadata?.name
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
      const validationErrors = validateEdgionGatewayConfig(source)
      if (validationErrors.length > 0) {
        message.error(t('msg.submitFailed', { err: validationErrors.join('; ') }))
        return
      }
      const yamlStr = toMutationYaml(source, mode === 'create' ? 'create' : 'update')
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
      ? t('modal.create', { resource: 'EdgionGatewayConfig' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'EdgionGatewayConfig' })
      : t('modal.view', { resource: 'EdgionGatewayConfig' })

  return (
    <Modal
      title={title}
      open={visible}
      onCancel={onClose}
      width={900}
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
      <Tabs
        activeKey={activeTab}
        onChange={handleTabChange}
        items={[
          {
            key: 'form',
            label: editorFormTab(t('tab.form')),
            children: (
              <EdgionGatewayConfigForm
                data={formData}
                onChange={setFormData}
                readOnly={isReadOnly}
                isCreate={mode === 'create'}
              />
            ),
          },
          {
            key: 'yaml',
            label: editorYamlTab(t('tab.yaml')),
            children: (
              <YamlEditor
                value={yamlContent}
                onChange={setYamlContent}
                readOnly={isReadOnly}
                height="500px"
              />
            ),
          },
          ...(mode !== 'create' ? [{
            key: 'conditions',
            label: t('tab.conditions'),
            children: <ResourceConditions status={formData.status} emptyText={t('status.noConditions')} />,
          }] : []),
        ]}
      />
    </Modal>
  )
}

export default EdgionGatewayConfigEditor
