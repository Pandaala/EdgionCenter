/**
 * 通用简化编辑器 — 只有 YAML 编辑器，无表单标签
 * 适用于：Service, EndpointSlice, GatewayClass, PluginMetaData, BackendTLSPolicy, ReferenceGrant 等
 */

import React, { useEffect, useState } from 'react'
import { Modal, Button, message } from 'antd'
import * as yaml from 'js-yaml'
import YamlEditor from '@/components/YamlEditor'
import type { K8sResource } from '@/api/types'
import { useT } from '@/i18n'

interface SimpleResourceEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: K8sResource | null
  title: string
  defaultYaml?: string
  onClose: () => void
  onSubmit: (yamlContent: string) => Promise<void>
  loading?: boolean
}

const SimpleResourceEditor: React.FC<SimpleResourceEditorProps> = ({
  visible,
  mode,
  resource,
  title,
  defaultYaml = '',
  onClose,
  onSubmit,
  loading = false,
}) => {
  const t = useT()
  const [yamlContent, setYamlContent] = useState('')

  useEffect(() => {
    if (!visible) return
    if (mode === 'create') {
      setYamlContent(defaultYaml)
    } else if (resource) {
      try {
        setYamlContent(yaml.dump(resource, { lineWidth: -1, noRefs: true }))
      } catch {
        setYamlContent('')
      }
    }
  }, [visible, mode, resource, defaultYaml])

  const handleSubmit = async () => {
    if (!yamlContent.trim()) {
      message.error(t('msg.yamlEmpty'))
      return
    }
    try {
      yaml.load(yamlContent)
    } catch (e: any) {
      message.error(t('msg.yamlInvalid', { err: e.message }))
      return
    }
    await onSubmit(yamlContent)
  }

  const modalTitle =
    mode === 'create'
      ? t('modal.create', { resource: title })
      : mode === 'edit'
      ? t('modal.edit', { resource: title })
      : t('modal.view', { resource: title })

  return (
    <Modal
      title={modalTitle}
      open={visible}
      onCancel={onClose}
      width={860}
      footer={
        mode === 'view'
          ? [<Button key="close" onClick={onClose}>{t('btn.close')}</Button>]
          : [
              <Button key="cancel" onClick={onClose}>{t('btn.cancel')}</Button>,
              <Button key="submit" type="primary" onClick={handleSubmit} loading={loading}>
                {mode === 'create' ? t('btn.create') : t('btn.save')}
              </Button>,
            ]
      }
    >
      <YamlEditor
        value={yamlContent}
        onChange={setYamlContent}
        readOnly={mode === 'view'}
        height="500px"
      />
    </Modal>
  )
}

export default SimpleResourceEditor
