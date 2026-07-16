import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import LinkSysForm from './LinkSysForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { LinkSys } from '@/types/link-sys'
import {
  createEmpty,
  normalize,
  toMutationYaml,
  toYaml,
  fromYaml,
  validateLinkSys,
} from '@/utils/linksys'
import { useT } from '@/i18n'
import ResourceConditions from '@/components/resource/ResourceConditions'

interface Props {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: LinkSys | null
  onClose: () => void
}

const LinkSysEditor: React.FC<Props> = ({ visible, mode, resource, onClose }) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [activeTab, setActiveTab] = useState<'form' | 'yaml' | 'conditions'>('form')
  const [formData, setFormData] = useState<LinkSys>(() => createEmpty())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()

  useEffect(() => {
    if (!visible) return
    setActiveTab('form')
    if (mode === 'create') { const e = createEmpty(); setFormData(e); setYamlContent(toYaml(e)) }
    else if (resource) { const n = normalize(resource); setFormData(n); setYamlContent(toYaml(n)) }
  }, [visible, mode, resource])

  const handleTabChange = (key: string) => {
    try {
      if (key === 'yaml') setYamlContent(toYaml(formData))
      else if (activeTab === 'yaml') setFormData(fromYaml(yamlContent))
      setActiveTab(key as 'form' | 'yaml' | 'conditions')
    } catch (e: any) { message.error(t('msg.tabSwitchFailed', { err: e.message })) }
  }

  const createMutation = useMutation({
    mutationFn: ({ namespace, y }: { namespace: string; y: string }) => resourceApi.create(mutationTarget, 'linksys', namespace, y),
    onSuccess: () => { message.success(t('msg.createOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'linksys'] }); onClose() },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })
  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, y }: { namespace: string; name: string; y: string }) => resourceApi.update(mutationTarget, 'linksys', namespace, name, y),
    onSuccess: () => { message.success(t('msg.updateOk')); queryClient.invalidateQueries({ queryKey: ['resource-list', 'linksys'] }); onClose() },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const sourceYaml = activeTab === 'yaml' ? yamlContent : toYaml(formData)
      const parsed = fromYaml(sourceYaml)
      validateLinkSys(parsed)
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
      const y = toMutationYaml(parsed, mode === 'create' ? 'create' : 'update')
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
      ? t('modal.create', { resource: 'LinkSys' })
      : mode === 'edit'
      ? t('modal.edit', { resource: 'LinkSys' })
      : t('modal.view', { resource: 'LinkSys' })

  return (
    <Modal title={title}
      open={visible} onCancel={onClose} width={820}
      destroyOnClose
      style={{ top: 20 }}
      footer={isRO ? [<Button key="close" onClick={onClose}>{t('btn.close')}</Button>] : [
        <Button {...editorCancelButtonProps} key="cancel" onClick={onClose}>{t('btn.cancel')}</Button>,
        <Button {...editorSubmitButtonProps} key="submit" type="primary" onClick={handleSubmit} loading={isPending}>
          {mode === 'create' ? t('btn.create') : t('btn.save')}
        </Button>,
      ]}
    >
      <Tabs activeKey={activeTab} onChange={handleTabChange} items={[
        { key: 'form', label: editorFormTab(t('tab.form')),
          children: <LinkSysForm data={formData} onChange={setFormData} readOnly={isRO} isCreate={mode === 'create'} /> },
        { key: 'yaml', label: editorYamlTab(t('tab.yaml')),
          children: <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={isRO} height="480px" /> },
        { key: 'conditions', label: 'Conditions', disabled: mode === 'create',
          children: <ResourceConditions status={resource?.status ?? formData.status} /> },
      ]} />
    </Modal>
  )
}

export default LinkSysEditor
