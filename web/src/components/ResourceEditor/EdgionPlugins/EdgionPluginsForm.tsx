/**
 * EdgionPlugins 表单组件
 * 元数据使用可编辑表单，插件配置以只读概览展示（编辑请用 YAML 模式）
 */

import React from 'react'
import { Form } from 'antd'
import MetadataSection from '@/components/ResourceEditor/HTTPRoute/sections/MetadataSection'
import PluginStagesSection from './sections/PluginStagesSection'
import type { EdgionPlugins } from '@/types/edgion-plugins'

interface EdgionPluginsFormProps {
  value: EdgionPlugins
  onChange: (value: EdgionPlugins) => void
  disabled?: boolean
  isCreate?: boolean
}

const EdgionPluginsForm: React.FC<EdgionPluginsFormProps> = ({
  value,
  onChange,
  disabled = false,
  isCreate = true,
}) => {
  return (
    <Form layout="vertical" style={{ maxHeight: '65vh', overflowY: 'auto', paddingRight: 8 }}>
      <MetadataSection
        value={value.metadata}
        onChange={(metadata) => onChange({ ...value, metadata })}
        disabled={disabled}
        isCreate={isCreate}
      />
      <div style={{ marginTop: 16 }}>
        <PluginStagesSection value={value.spec} />
      </div>
    </Form>
  )
}

export default EdgionPluginsForm
