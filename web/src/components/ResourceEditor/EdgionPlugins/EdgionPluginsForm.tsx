/**
 * EdgionPlugins 表单组件
 * 元数据使用可编辑表单，插件配置以只读概览展示（编辑请用 YAML 模式）
 */

import React from 'react'
import { Alert, Button, Card, Form, Input, Select, Space } from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import MetadataSection from '@/components/ResourceEditor/HTTPRoute/sections/MetadataSection'
import PluginStagesSection from './sections/PluginStagesSection'
import type {
  AccessLogExternField,
  AccessLogExternSource,
  EdgionPlugins,
} from '@/types/edgion-plugins'
import { validateAccessLogExtern } from '@/utils/edgionplugins'
import { useT } from '@/i18n'

interface EdgionPluginsFormProps {
  value: EdgionPlugins
  onChange: (value: EdgionPlugins) => void
  disabled?: boolean
  isCreate?: boolean
}

const ACCESS_LOG_SOURCES: AccessLogExternSource[] = [
  'routeLabel',
  'routeAnnotation',
  'header',
  'query',
  'cookie',
  'respHeader',
  'ctx',
]

function AccessLogExternSection({ value, onChange, readOnly }: {
  value: AccessLogExternField[]
  onChange: (value: AccessLogExternField[]) => void
  readOnly: boolean
}) {
  const t = useT()
  const errors = validateAccessLogExtern(value)
  return (
    <Card title={t('plugins.accessLogExtern')} size="small" style={{ marginTop: 16 }}>
      <Space direction="vertical" style={{ width: '100%' }}>
        {errors.length > 0 && (
          <Alert
            type="error"
            showIcon
            message={t('plugins.accessLogExternInvalid')}
            description={errors.join('\n')}
          />
        )}
        {value.map((field, index) => (
          <Card key={index} size="small">
            <Space wrap align="start">
              <Form.Item label={t('plugins.accessLogKey')} required>
                <Input
                  value={field.key}
                  disabled={readOnly}
                  onChange={(event) => {
                    const next = [...value]
                    next[index] = { ...field, key: event.target.value }
                    onChange(next)
                  }}
                />
              </Form.Item>
              <Form.Item label={t('plugins.accessLogSource')} required>
                <Select
                  value={field.from}
                  disabled={readOnly}
                  style={{ width: 180 }}
                  options={ACCESS_LOG_SOURCES.map((source) => ({ value: source, label: source }))}
                  onChange={(from) => {
                    const next = [...value]
                    next[index] = { ...field, from }
                    onChange(next)
                  }}
                />
              </Form.Item>
              <Form.Item label={t('plugins.accessLogName')} required>
                <Input
                  value={field.name}
                  disabled={readOnly}
                  onChange={(event) => {
                    const next = [...value]
                    next[index] = { ...field, name: event.target.value }
                    onChange(next)
                  }}
                />
              </Form.Item>
              {!readOnly && (
                <Button
                  danger
                  icon={<DeleteOutlined />}
                  onClick={() => onChange(value.filter((_, current) => current !== index))}
                >
                  {t('btn.delete')}
                </Button>
              )}
            </Space>
          </Card>
        ))}
        {!readOnly && (
          <Button
            type="dashed"
            block
            icon={<PlusOutlined />}
            disabled={value.length >= 16}
            onClick={() => onChange([...value, { key: '', from: 'header', name: '' }])}
          >
            {t('plugins.addAccessLogExtern')}
          </Button>
        )}
      </Space>
    </Card>
  )
}

const EdgionPluginsForm: React.FC<EdgionPluginsFormProps> = ({
  value,
  onChange,
  disabled = false,
  isCreate = true,
}) => {
  const accessLogExtern = value.spec.accessLogExtern ?? []
  return (
    <Form layout="vertical" style={{ maxHeight: '65vh', overflowY: 'auto', paddingRight: 8 }}>
      <MetadataSection
        value={value.metadata}
        onChange={(metadata) => onChange({ ...value, metadata })}
        disabled={disabled}
        isCreate={isCreate}
      />
      <div style={{ marginTop: 16 }}>
        <PluginStagesSection
          value={value.spec}
          onChange={(spec) => onChange({ ...value, spec })}
          readOnly={disabled}
        />
        <AccessLogExternSection
          value={accessLogExtern}
          onChange={(next) => onChange({
            ...value,
            spec: { ...value.spec, accessLogExtern: next },
          })}
          readOnly={disabled}
        />
      </div>
    </Form>
  )
}

export default EdgionPluginsForm
