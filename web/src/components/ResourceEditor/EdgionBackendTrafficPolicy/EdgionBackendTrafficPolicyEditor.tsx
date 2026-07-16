import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useEffect, useState } from 'react'
import { Alert, Button, Modal, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import {
  createEmptyEdgionBackendTrafficPolicy,
  edgionBackendTrafficPolicyFromYaml,
  edgionBackendTrafficPolicyToMutationYaml,
  edgionBackendTrafficPolicyToYaml,
  normalizeEdgionBackendTrafficPolicy,
  validateEdgionBackendTrafficPolicy,
} from '@/utils/edgionbackendtrafficpolicy'
import YamlEditor from '@/components/YamlEditor'
import ResourceConditions from '@/components/resource/ResourceConditions'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { useT } from '@/i18n'
import EdgionBackendTrafficPolicyForm from './EdgionBackendTrafficPolicyForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import { useEditorTabTransition } from '../useEditorTabTransition'

interface Props {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: unknown | null
  onClose: () => void
}

const RESOURCE_KIND = 'edgionbackendtrafficpolicy' as const

const EdgionBackendTrafficPolicyEditor = ({ visible, mode, resource, onClose }: Props) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState(createEmptyEdgionBackendTrafficPolicy)
  const [yamlContent, setYamlContent] = useState('')
  const [formDraftErrors, setFormDraftErrors] = useState<string[]>([])
  const queryClient = useQueryClient()
  const { activeTab, editableTab, resetEditorTab, handleTabChange } = useEditorTabTransition({
    formData,
    yamlContent,
    serialize: (value) => {
      if (formDraftErrors.length > 0) throw new Error(formDraftErrors.join('; '))
      return edgionBackendTrafficPolicyToYaml(value)
    },
    parse: edgionBackendTrafficPolicyFromYaml,
    setFormData,
    setYamlContent,
    onError: (error) => message.error(t('msg.tabSwitchFailed', { err: error.message })),
  })

  useEffect(() => {
    if (!visible) return
    const next = mode === 'create'
      ? createEmptyEdgionBackendTrafficPolicy()
      : normalizeEdgionBackendTrafficPolicy(resource)
    resetEditorTab()
    setFormDraftErrors([])
    setFormData(next)
    setYamlContent(edgionBackendTrafficPolicyToYaml(next))
  }, [visible, mode, resource, resetEditorTab])

  const createMutation = useMutation({
    mutationFn: ({ namespace, source }: { namespace: string; source: string }) => resourceApi.create(mutationTarget, RESOURCE_KIND, namespace, source),
    onSuccess: () => {
      message.success(t('msg.createOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', RESOURCE_KIND] })
      onClose()
    },
    onError: (error: Error) => message.error(t('msg.createFailed', { err: error.message })),
  })
  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, source }: { namespace: string; name: string; source: string }) => resourceApi.update(mutationTarget, RESOURCE_KIND, namespace, name, source),
    onSuccess: () => {
      message.success(t('msg.updateOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', RESOURCE_KIND] })
      onClose()
    },
    onError: (error: Error) => message.error(t('msg.updateFailed', { err: error.message })),
  })

  const submit = () => {
    try {
      const source = editableTab === 'form' ? formData : edgionBackendTrafficPolicyFromYaml(yamlContent)
      const { name, namespace } = source.metadata
      if (!name || !namespace) throw new Error(t('msg.metaRequired'))
      if (mode === 'edit' && resource && typeof resource === 'object') {
        const original = normalizeEdgionBackendTrafficPolicy(resource)
        if (name !== original.metadata.name || namespace !== original.metadata.namespace) throw new Error(t('msg.noRename'))
      }
      const validationErrors = validateEdgionBackendTrafficPolicy(source)
      const allErrors = editableTab === 'form' ? [...formDraftErrors, ...validationErrors] : validationErrors
      if (allErrors.length) throw new Error(allErrors.join('; '))
      const mutationYaml = edgionBackendTrafficPolicyToMutationYaml(source, mode === 'create' ? 'create' : 'update')
      if (mode === 'create') createMutation.mutate({ namespace, source: mutationYaml })
      else updateMutation.mutate({ namespace, name, source: mutationYaml })
    } catch (error) {
      message.error(t('msg.submitFailed', { err: error instanceof Error ? error.message : String(error) }))
    }
  }

  const title = t(`modal.${mode}`, { resource: 'EdgionBackendTrafficPolicy' })
  const pending = createMutation.isPending || updateMutation.isPending

  return (
    <Modal
      title={title}
      open={visible}
      onCancel={onClose}
      width={1000}
      destroyOnClose
      style={{ top: 20 }}
      footer={mode === 'view' ? [
        <Button key="close" onClick={onClose}>{t('btn.close')}</Button>,
      ] : [
        <Button {...editorCancelButtonProps} key="cancel" onClick={onClose}>{t('btn.cancel')}</Button>,
        <PermissionAwareButton
          {...editorSubmitButtonProps}
          key="submit"
          type="primary"
          resourceKind={RESOURCE_KIND}
          resourceVerb={mode === 'create' ? 'create' : 'update'}
          loading={pending}
          onClick={submit}
        >
          {mode === 'create' ? t('btn.create') : t('btn.save')}
        </PermissionAwareButton>,
      ]}
    >
      {mode !== 'view' && validateEdgionBackendTrafficPolicy(formData).length > 0 && editableTab === 'form' && (
        <Alert
          style={{ marginBottom: 12 }}
          type="warning"
          showIcon
          message={t('notice.completeRequiredFields')}
        />
      )}
      <Tabs activeKey={activeTab} onChange={handleTabChange} items={[
        {
          key: 'form',
          label: editorFormTab(t('tab.form')),
          children: (
            <EdgionBackendTrafficPolicyForm
              data={formData}
              onChange={setFormData}
              readOnly={mode === 'view'}
              isCreate={mode === 'create'}
              onDraftValidationChange={setFormDraftErrors}
            />
          ),
        },
        {
          key: 'yaml',
          label: editorYamlTab('YAML'),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={mode === 'view'} height="600px" />,
        },
        ...(mode !== 'create' ? [{
          key: 'conditions',
          label: t('tab.conditions'),
          children: <ResourceConditions status={formData.status} emptyText={t('status.noConditions')} />,
        }] : []),
      ]} />
    </Modal>
  )
}

export default EdgionBackendTrafficPolicyEditor
