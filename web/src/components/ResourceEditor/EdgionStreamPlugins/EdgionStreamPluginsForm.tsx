/**
 * EdgionStreamPlugins 表单
 */

import React from 'react'
import { Form, Input, Select, Button, Card, Space } from 'antd'
import { PlusOutlined, MinusCircleOutlined } from '@ant-design/icons'
import MetadataSection from '../common/MetadataSection'
import type { EdgionStreamPlugins, StreamPlugin } from '@/types/edgion-stream-plugins'
import { useT } from '@/i18n'

interface EdgionStreamPluginsFormProps {
  data: EdgionStreamPlugins
  onChange: (data: EdgionStreamPlugins) => void
  readOnly?: boolean
  isCreate?: boolean
}

const defaultPlugin = (): StreamPlugin => ({
  type: 'IpRestriction',
  config: { ipSource: 'remoteAddr', allow: [], deny: [], defaultAction: 'allow', message: '' },
})

const EdgionStreamPluginsForm: React.FC<EdgionStreamPluginsFormProps> = ({
  data, onChange, readOnly = false, isCreate = true,
}) => {
  const t = useT()
  const plugins = data.spec?.plugins || []

  const updatePlugin = (index: number, plugin: StreamPlugin) => {
    const next = [...plugins]
    next[index] = plugin
    onChange({ ...data, spec: { ...data.spec, plugins: next } })
  }

  const addPlugin = () =>
    onChange({ ...data, spec: { ...data.spec, plugins: [...plugins, defaultPlugin()] } })

  const removePlugin = (index: number) =>
    onChange({ ...data, spec: { ...data.spec, plugins: plugins.filter((_, i) => i !== index) } })

  const updateConfig = (index: number, field: string, value: any) => {
    const plugin = plugins[index]
    updatePlugin(index, { ...plugin, config: { ...(plugin.config || {}), [field]: value } })
  }

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        {plugins.map((plugin, index) => (
          <Card
            key={index}
            title={`${t('col.plugins')} ${index + 1}: ${plugin.type}`}
            size="small"
            extra={!readOnly && plugins.length > 1 && (
              <Button danger size="small" icon={<MinusCircleOutlined />} onClick={() => removePlugin(index)}>{t('btn.delete')}</Button>
            )}
          >
            <Form.Item label={t('field.pluginType')} style={{ marginBottom: 8 }}>
              <Select
                value={plugin.type}
                onChange={(v) => updatePlugin(index, { ...plugin, type: v })}
                disabled={readOnly}
                style={{ width: 200 }}
              >
                <Select.Option value="IpRestriction">{t('sp.ipRestriction')}</Select.Option>
              </Select>
            </Form.Item>

            {plugin.type === 'IpRestriction' && (
              <>
                <Form.Item label={t('field.ipSource')} style={{ marginBottom: 8 }}>
                  <Select
                    value={(plugin.config as any)?.ipSource || 'remoteAddr'}
                    onChange={(v) => updateConfig(index, 'ipSource', v)}
                    disabled={readOnly}
                    style={{ width: 200 }}
                  >
                    <Select.Option value="remoteAddr">{t('sp.remoteAddr')}</Select.Option>
                  </Select>
                </Form.Item>
                <Form.Item label={t('field.allowList')} style={{ marginBottom: 8 }}>
                  <Select
                    mode="tags"
                    value={(plugin.config as any)?.allow || []}
                    onChange={(v) => updateConfig(index, 'allow', v)}
                    disabled={readOnly}
                    placeholder="10.0.0.0/8"
                    style={{ width: '100%' }}
                  />
                </Form.Item>
                <Form.Item label={t('field.denyList')} style={{ marginBottom: 8 }}>
                  <Select
                    mode="tags"
                    value={(plugin.config as any)?.deny || []}
                    onChange={(v) => updateConfig(index, 'deny', v)}
                    disabled={readOnly}
                    placeholder="10.0.0.100/32"
                    style={{ width: '100%' }}
                  />
                </Form.Item>
                <Form.Item label={t('field.defaultAction')} style={{ marginBottom: 8 }}>
                  <Select
                    value={(plugin.config as any)?.defaultAction || 'allow'}
                    onChange={(v) => updateConfig(index, 'defaultAction', v)}
                    disabled={readOnly}
                    style={{ width: 160 }}
                  >
                    <Select.Option value="allow">{t('sp.allow')}</Select.Option>
                    <Select.Option value="deny">{t('sp.deny')}</Select.Option>
                  </Select>
                </Form.Item>
                <Form.Item label={t('field.rejectMsg')} style={{ marginBottom: 0 }}>
                  <Input
                    value={(plugin.config as any)?.message || ''}
                    onChange={(e) => updateConfig(index, 'message', e.target.value)}
                    placeholder="Access denied"
                    disabled={readOnly}
                  />
                </Form.Item>
              </>
            )}
          </Card>
        ))}

        {!readOnly && (
          <Button type="dashed" block icon={<PlusOutlined />} onClick={addPlugin}>
            {t('btn.addPlugin')}
          </Button>
        )}
      </Space>
    </Form>
  )
}

export default EdgionStreamPluginsForm
