import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import EdgionStreamPluginsForm from './EdgionStreamPluginsForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { EdgionStreamPlugins } from '@/types/edgion-stream-plugins'
import { createEmpty, normalize, toYaml, fromYaml, toMutationDocument } from '@/utils/edgionstreamplugins'
import { dumpYaml } from '@/utils/yaml-utils'
import { useT } from '@/i18n'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { useEditorTabTransition } from '../useEditorTabTransition'

interface Props {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: EdgionStreamPlugins | null
  onClose: () => void
}

const EdgionStreamPluginsEditor: React.FC<Props> = ({ visible, mode, resource, onClose }) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState<EdgionStreamPlugins>(() => createEmpty())
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
  const queryClient = useQueryClient()

  useEffect(() => {
    if (!visible) return
    resetEditorTab()
    if (mode === 'create') { const e = createEmpty(); setFormData(e); setYamlContent(toYaml(e)) }
    else if (resource) { const n = normalize(resource); setFormData(n); setYamlContent(toYaml(n)) }
  }, [visible, mode, resource, resetEditorTab])

  const createMutation = useMutation({
    mutationFn: ({ namespace, y }: { namespace: string; y: string }) => resourceApi.create(mutationTarget, 'edgionstreamplugins', namespace, y),
    onSuccess: () => { message.success(t('msg.createOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionstreamplugins'] }); onClose() },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })
  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, y }: { namespace: string; name: string; y: string }) => resourceApi.update(mutationTarget, 'edgionstreamplugins', namespace, name, y),
    onSuccess: () => { message.success(t('msg.updateOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionstreamplugins'] }); onClose() },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const parsed = editableTab === 'yaml' ? fromYaml(yamlContent) : formData
      const y = dumpYaml(toMutationDocument(parsed, mode === 'create' ? 'create' : 'update'))
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
      if (mode === 'create') createMutation.mutate({ namespace, y })
      else updateMutation.mutate({ namespace, name, y })
    } catch (e: any) {
      message.error(t('msg.submitFailed', { err: e.message || 'unknown error' }))
    }
  }

  const isPending = createMutation.isPending || updateMutation.isPending
  const isRO = mode === 'view'

  const title =
    mode === 'create'
      ? t('modal.create', { resource: 'EdgionStreamPlugins' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'EdgionStreamPlugins' })
      : t('modal.view', { resource: 'EdgionStreamPlugins' })

  return (
    <Modal title={title}
      open={visible} onCancel={onClose} width={820}
      destroyOnClose
      style={{ top: 20 }}
      footer={isRO ? [<Button key="close" onClick={onClose}>{t('btn.close')}</Button>] : [
        <Button {...editorCancelButtonProps} key="cancel" onClick={onClose}>{t('btn.cancel')}</Button>,
        <PermissionAwareButton {...editorSubmitButtonProps} key="submit" type="primary" resourceKind="edgionstreamplugins" resourceVerb={mode === 'create' ? 'create' : 'update'} onClick={handleSubmit} loading={isPending}>
          {mode === 'create' ? t('btn.create') : t('btn.save')}
        </PermissionAwareButton>,
      ]}
    >
      <Tabs activeKey={activeTab} onChange={handleTabChange} items={[
        { key: 'form', label: editorFormTab(t('tab.form')),
          children: <EdgionStreamPluginsForm data={formData} onChange={setFormData} readOnly={isRO} isCreate={mode === 'create'} /> },
        { key: 'yaml', label: editorYamlTab(t('tab.yaml')),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={isRO} height="480px" /> },
        ...(mode !== 'create' ? [{ key: 'conditions', label: t('tab.conditions'), children: <ResourceConditions status={formData.status} emptyText={t('status.noConditions')} /> }] : []),
      ]} />
    </Modal>
  )
}

export default EdgionStreamPluginsEditor
