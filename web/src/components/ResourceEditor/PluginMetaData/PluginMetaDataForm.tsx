/**
 * PluginMetaData 表单
 */

import React from 'react'
import { Form, Input, Card, Space, Alert } from 'antd'
import MetadataSection from '../common/MetadataSection'
import type { PluginMetaDataResource } from '@/utils/pluginmetadata'
import { useT } from '@/i18n'

interface PluginMetaDataFormProps {
  data: PluginMetaDataResource
  onChange: (data: PluginMetaDataResource) => void
  readOnly?: boolean
  isCreate?: boolean
}

const PluginMetaDataForm: React.FC<PluginMetaDataFormProps> = ({
  data,
  onChange,
  readOnly = false,
  isCreate = true,
}) => {
  const t = useT()

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        <Card title={t('section.pluginInfo')} size="small">
          <Form.Item label={t('field.pluginDesc')} style={{ marginBottom: 8 }}>
            <Input.TextArea
              value={data.spec?.description || ''}
              onChange={(e) =>
                onChange({ ...data, spec: { ...data.spec, description: e.target.value || undefined } })
              }
              rows={2}
              disabled={readOnly}
            />
          </Form.Item>
          <Alert
            type="info"
            showIcon
            message={t('notice.pluginMetaYaml')}
            style={{ marginBottom: 0 }}
          />
        </Card>
      </Space>
    </Form>
  )
}

export default PluginMetaDataForm
