/**
 * EdgionConfigData editor modal
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import EdgionConfigDataForm from './EdgionConfigDataForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { EdgionConfigDataResource } from '@/utils/edgionConfigData'
import { createEmpty, normalize, toMutationYaml, toYaml, fromYaml } from '@/utils/edgionConfigData'
import { useT } from '@/i18n'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { useEditorTabTransition } from '../useEditorTabTransition'

interface EdgionConfigDataEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: any | null
  onClose: () => void
}

const EdgionConfigDataEditor: React.FC<EdgionConfigDataEditorProps> = ({
  visible,
  mode,
  resource,
  onClose,
}) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState<EdgionConfigDataResource>(() => createEmpty())
  const [yamlContent, setYamlContent] = useState('')
  const { activeTab, editableTab, resetEditorTab, handleTabChange } = useEditorTabTransition({
    formData,
    yamlContent,
    serialize: toYaml,
    parse: fromYaml,
    setFormData,
    setYamlContent,
    onError: (error) => message.error(t('msg.tabSwitchFailed', { err: error.message })),
  })
  const [formValid, setFormValid] = useState(true)
  const queryClient = useQueryClient()

  useEffect(() => {
    if (!visible) return
    resetEditorTab()
    setFormValid(true)
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
    mutationFn: ({ namespace, yamlStr }: { namespace: string; yamlStr: string }) =>
      resourceApi.create(mutationTarget, 'edgionconfigdata', namespace, yamlStr),
    onSuccess: () => {
      message.success(t('msg.createOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionconfigdata'] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, yamlStr }: { namespace: string; name: string; yamlStr: string }) =>
      resourceApi.update(mutationTarget, 'edgionconfigdata', namespace, name, yamlStr),
    onSuccess: () => {
      message.success(t('msg.updateOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionconfigdata'] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const candidate = editableTab === 'form' ? formData : fromYaml(yamlContent)
      const mutationMode = mode === 'create' ? 'create' : 'update'
      const name = candidate.metadata?.name
      const namespace = candidate.metadata?.namespace
      const yamlStr = toMutationYaml(candidate, mutationMode)
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
      ? t('modal.create', { resource: 'EdgionConfigData' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'EdgionConfigData' })
      : t('modal.view', { resource: 'EdgionConfigData' })

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
              <PermissionAwareButton {...editorSubmitButtonProps} key="submit" type="primary" resourceKind="edgionconfigdata" resourceVerb={mode === 'create' ? 'create' : 'update'} onClick={handleSubmit} loading={isPending} disabled={editableTab === 'form' && !formValid}>
                {mode === 'create' ? t('btn.create') : t('btn.save')}
              </PermissionAwareButton>,
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
              <EdgionConfigDataForm
                data={formData}
                onChange={setFormData}
                readOnly={isReadOnly}
                isCreate={mode === 'create'}
                onValidityChange={setFormValid}
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

export default EdgionConfigDataEditor
