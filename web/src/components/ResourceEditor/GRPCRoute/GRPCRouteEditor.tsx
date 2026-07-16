/**
 * GRPCRoute 编辑器 Modal
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import ResourceConditions from '@/components/resource/ResourceConditions'
import GRPCRouteForm from './GRPCRouteForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { GRPCRoute } from '@/types/gateway-api/grpcroute'
import {
  createEmptyGRPCRoute,
  normalizeGRPCRoute,
  grpcRouteToMutationYaml,
  grpcRouteToYaml,
  yamlToGRPCRoute,
} from '@/utils/grpcroute'
import { useT } from '@/i18n'
import { useEditorTabTransition } from '../useEditorTabTransition'

interface GRPCRouteEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: GRPCRoute | null
  onClose: () => void
}

const GRPCRouteEditor: React.FC<GRPCRouteEditorProps> = ({
  visible, mode, resource, onClose,
}) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState<GRPCRoute>(() => createEmptyGRPCRoute())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()
  const { activeTab, editableTab, resetEditorTab, handleTabChange } = useEditorTabTransition({
    formData, yamlContent, serialize: grpcRouteToYaml, parse: yamlToGRPCRoute, setFormData, setYamlContent,
    onError: (error) => message.error(t('msg.tabSwitchFailed', { err: error.message })),
  })

  useEffect(() => {
    if (!visible) return
    resetEditorTab()
    if (mode === 'create') {
      const empty = createEmptyGRPCRoute()
      setFormData(empty)
      setYamlContent(grpcRouteToYaml(empty))
    } else if (resource) {
      const normalized = normalizeGRPCRoute(resource)
      setFormData(normalized)
      setYamlContent(grpcRouteToYaml(normalized))
    }
  }, [visible, mode, resource, resetEditorTab])

  const createMutation = useMutation({
    mutationFn: ({ namespace, yamlStr }: { namespace: string; yamlStr: string }) =>
      resourceApi.create(mutationTarget, 'grpcroute', namespace, yamlStr),
    onSuccess: () => {
      message.success(t('msg.createOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'grpcroute'] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, yamlStr }: { namespace: string; name: string; yamlStr: string }) =>
      resourceApi.update(mutationTarget, 'grpcroute', namespace, name, yamlStr),
    onSuccess: () => {
      message.success(t('msg.updateOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'grpcroute'] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const sourceYaml = editableTab === 'yaml' ? yamlContent : grpcRouteToYaml(formData)
      const parsed = yamlToGRPCRoute(sourceYaml)
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
      const yamlStr = grpcRouteToMutationYaml(parsed, mode === 'create' ? 'create' : 'update')
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
      ? t('modal.create', { resource: 'GRPCRoute' })
      : mode === 'edit'
      ? t('modal.edit', { resource: resource?.metadata.name || 'GRPCRoute' })
      : t('modal.view', { resource: resource?.metadata.name || 'GRPCRoute' })

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
              <GRPCRouteForm
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
          ...(mode !== 'create' ? [{ key: 'conditions', label: t('tab.conditions'), children: <ResourceConditions status={formData.status} emptyText={t('status.noConditions')} /> }] : []),
        ]}
      />
    </Modal>
  )
}

export default GRPCRouteEditor
