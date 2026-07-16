import React, { useEffect } from 'react'
import { Card, Form, Input, Select, Space, Switch } from 'antd'
import MetadataSection from '../common/MetadataSection'
import StructuredConfigEditor from '../EdgionPlugins/StructuredConfigEditor'
import type { PluginField } from '../EdgionPlugins/pluginCatalog'
import type {
  ConfigDataType,
  EdgionConfigDataResource,
} from '@/utils/edgionConfigData'
import { replaceConfigDataType } from '@/utils/edgionConfigData'
import { useT } from '@/i18n'

interface EdgionConfigDataFormProps {
  data: EdgionConfigDataResource
  onChange: (data: EdgionConfigDataResource) => void
  readOnly?: boolean
  isCreate?: boolean
  onValidityChange?: (valid: boolean) => void
}

const DATA_TYPES: ConfigDataType[] = [
  'KeyList',
  'IpList',
  'Selector',
  'RegionRouteOverride',
  'Misc',
]

const CONFIG_FIELDS: Record<ConfigDataType, readonly PluginField[]> = {
  KeyList: [
    { name: 'matchMode', kind: 'string', options: ['exact', 'regex'] },
    { name: 'items', kind: 'array' },
  ],
  IpList: [{ name: 'items', kind: 'array' }],
  Selector: [{ name: 'active', kind: 'string' }, { name: 'description', kind: 'string' }],
  RegionRouteOverride: [
    { name: 'enable', kind: 'boolean' }, { name: 'active', kind: 'string' },
    { name: 'regions', kind: 'array' },
  ],
  Misc: [],
}

const EdgionConfigDataForm: React.FC<EdgionConfigDataFormProps> = ({
  data,
  onChange,
  readOnly = false,
  isCreate = true,
  onValidityChange = () => undefined,
}) => {
  const t = useT()
  useEffect(() => onValidityChange(true), [onValidityChange])
  const updateSpec = (patch: Partial<EdgionConfigDataResource['spec']>) => {
    onChange({ ...data, spec: { ...data.spec, ...patch } })
  }

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata: { ...data.metadata, ...metadata } })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        <Card title={t('section.configDataEnvelope')} size="small">
          <Form.Item label={t('field.enabled')} style={{ marginBottom: 8 }}>
            <Switch
              checked={data.spec.enable}
              onChange={(enable) => updateSpec({ enable })}
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item label={t('field.activeProfile')} style={{ marginBottom: 8 }}>
            <Input
              value={data.spec.active ?? ''}
              onChange={(event) => updateSpec({ active: event.target.value || undefined })}
              disabled={readOnly}
              allowClear
            />
          </Form.Item>
          <Form.Item label={t('field.visibility')} style={{ marginBottom: 0 }}>
            <Select
              value={data.spec.visibility}
              onChange={(visibility) => updateSpec({ visibility })}
              disabled={readOnly}
              options={[
                { value: 'Namespace', label: 'Namespace' },
                { value: 'Cluster', label: 'Cluster' },
              ]}
            />
          </Form.Item>
        </Card>

        <Card title={t('section.configDataPayload')} size="small">
          <Form.Item label={t('field.configDataType')} required style={{ marginBottom: 8 }}>
            <Select
              value={data.spec.data.type}
              onChange={(type) => onChange(replaceConfigDataType(data, type))}
              disabled={readOnly}
              options={DATA_TYPES.map((type) => ({ value: type, label: type }))}
            />
          </Form.Item>
          <StructuredConfigEditor
            fields={CONFIG_FIELDS[data.spec.data.type]}
            value={data.spec.data.config}
            onChange={(config) => updateSpec({ data: { ...data.spec.data, config } })}
            readOnly={readOnly}
          />
        </Card>
      </Space>
    </Form>
  )
}

export default EdgionConfigDataForm
