/**
 * EdgionPlugins 主编辑器
 * 支持表单模式（元数据 + 插件概览）和 YAML 模式切换，带双向同步
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useState, useEffect } from 'react'
import { Modal, Tabs, Button, Space, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import EdgionPluginsForm from './EdgionPluginsForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import YamlEditor from '@/components/YamlEditor'
import { resourceApi } from '@/api/resources'
import {
  createEmptyEdgionPlugins,
  normalizeEdgionPlugins,
  edgionPluginsToYAML,
  edgionPluginsToMutationYAML,
  yamlToEdgionPlugins,
} from '@/utils/edgionplugins'
import type { EdgionPlugins } from '@/types/edgion-plugins'
import type { K8sResource } from '@/api/types'
import { useT } from '@/i18n'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { useEditorTabTransition } from '../useEditorTabTransition'


interface EdgionPluginsEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: K8sResource | null
  onClose: () => void
}

const EdgionPluginsEditor: React.FC<EdgionPluginsEditorProps> = ({
  visible,
  mode: initialMode,
  resource,
  onClose,
}) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState<EdgionPlugins | null>(null)
  const [yamlContent, setYamlContent] = useState<string>('')
  const queryClient = useQueryClient()
  const { activeTab, editableTab, resetEditorTab, handleTabChange } = useEditorTabTransition({
    formData,
    yamlContent,
    serialize: (value) => {
      if (!value) throw new Error(t('msg.formEmpty'))
      return edgionPluginsToYAML(normalizeEdgionPlugins(value))
    },
    parse: (source) => normalizeEdgionPlugins(yamlToEdgionPlugins(source)),
    setFormData,
    setYamlContent,
    onError: (error) => message.error(t('msg.tabSwitchFailed', { err: error.message })),
  })

  const isReadOnly = initialMode === 'view'

  // 初始化数据
  useEffect(() => {
    if (visible) {
      if (resource) {
        const normalized = normalizeEdgionPlugins(resource as unknown as EdgionPlugins)
        setFormData(normalized)
        setYamlContent(edgionPluginsToYAML(normalized))
      } else {
        const empty = createEmptyEdgionPlugins()
        setFormData(empty)
        setYamlContent(edgionPluginsToYAML(empty))
      }
      resetEditorTab()
    }
  }, [visible, initialMode, resource, resetEditorTab])

  // 表单 → YAML 同步
  const handleFormChange = (newFormData: EdgionPlugins) => {
    setFormData(newFormData)
    try {
      const normalized = normalizeEdgionPlugins(newFormData)
      setYamlContent(edgionPluginsToYAML(normalized))
    } catch (e) {
      console.error('Form to YAML conversion error:', e)
    }
  }

  // 创建 Mutation
  const createMutation = useMutation({
    mutationFn: ({ namespace, content }: { namespace: string; name: string; content: string }) =>
      resourceApi.create(mutationTarget, 'edgionplugins', namespace, content),
    onSuccess: () => {
      message.success(t('msg.createOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionplugins'] })
      onClose()
    },
    onError: (error: any) => {
      message.error(t('msg.createFailed', { err: error.message }))
    },
  })

  // 更新 Mutation
  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, content }: { namespace: string; name: string; content: string }) =>
      resourceApi.update(mutationTarget, 'edgionplugins', namespace, name, content),
    onSuccess: () => {
      message.success(t('msg.updateOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'edgionplugins'] })
      onClose()
    },
    onError: (error: any) => {
      message.error(t('msg.updateFailed', { err: error.message }))
    },
  })

  // 提交处理
  const handleSubmit = async () => {
    try {
      let parsedResource: EdgionPlugins
      let contentToSubmit: string

      if (editableTab === 'form') {
        if (!formData) {
          message.error(t('msg.formEmpty'))
          return
        }
        parsedResource = normalizeEdgionPlugins(formData)
        contentToSubmit = edgionPluginsToMutationYAML(parsedResource, initialMode === 'create' ? 'create' : 'update')
      } else {
        parsedResource = yamlToEdgionPlugins(yamlContent)
        contentToSubmit = edgionPluginsToMutationYAML(parsedResource, initialMode === 'create' ? 'create' : 'update')
      }

      const name = parsedResource.metadata?.name
      const namespace = parsedResource.metadata?.namespace

      if (!name || !namespace) {
        message.error(t('msg.metaRequired'))
        return
      }

      if (initialMode === 'create') {
        createMutation.mutate({ namespace, name, content: contentToSubmit })
      } else if (resource) {
        if (name !== resource.metadata.name || namespace !== resource.metadata.namespace) {
          message.error(t('msg.noRename'))
          return
        }
        updateMutation.mutate({ namespace, name, content: contentToSubmit })
      }
    } catch (e: any) {
      message.error(t('msg.submitFailed', { err: e.message || 'unknown error' }))
    }
  }

  const title =
    initialMode === 'create'
      ? t('modal.create', { resource: 'EdgionPlugins' })
      : initialMode === 'edit'
      ? t('modal.edit', { resource: resource?.metadata.name || 'EdgionPlugins' })
      : t('modal.view', { resource: resource?.metadata.name || 'EdgionPlugins' })

  const footer = (
    <Space>
      <Button {...editorCancelButtonProps} onClick={onClose}>
        {initialMode === 'view' ? t('btn.close') : t('btn.cancel')}
      </Button>
      {initialMode !== 'view' && (
        <PermissionAwareButton
          {...editorSubmitButtonProps}
          type="primary"
          resourceKind="edgionplugins"
          resourceVerb={initialMode === 'create' ? 'create' : 'update'}
          onClick={handleSubmit}
          loading={createMutation.isPending || updateMutation.isPending}
        >
          {initialMode === 'create' ? t('btn.create') : t('btn.save')}
        </PermissionAwareButton>
      )}
    </Space>
  )

  return (
    <Modal
      title={title}
      open={visible}
      onCancel={onClose}
      footer={footer}
      width={900}
      destroyOnClose
      style={{ top: 20 }}
    >
      <Tabs activeKey={activeTab} onChange={handleTabChange} items={[
        {
          key: 'form',
          label: editorFormTab(t('tab.form')),
          children: formData && (
            <EdgionPluginsForm
              value={formData}
              onChange={handleFormChange}
              disabled={isReadOnly}
              isCreate={initialMode === 'create'}
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
              height="65vh"
            />
          ),
        },
        ...(initialMode !== 'create' ? [{
          key: 'conditions',
          label: t('tab.conditions'),
          children: <ResourceConditions status={formData?.status} emptyText={t('status.noConditions')} />,
        }] : []),
      ]} />
    </Modal>
  )
}

export default EdgionPluginsEditor
