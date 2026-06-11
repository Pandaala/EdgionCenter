/**
 * ReferenceGrant 表单
 */

import React from 'react'
import { Form, Input, Card, Space, Button } from 'antd'
import { PlusOutlined, MinusCircleOutlined } from '@ant-design/icons'
import MetadataSection from '../common/MetadataSection'
import type { ReferenceGrant, ReferenceGrantFrom, ReferenceGrantTo } from '@/utils/referencegrant'
import { useT } from '@/i18n'

interface ReferenceGrantFormProps {
  data: ReferenceGrant
  onChange: (data: ReferenceGrant) => void
  readOnly?: boolean
  isCreate?: boolean
}

const ReferenceGrantForm: React.FC<ReferenceGrantFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()

  const updateFromRef = (index: number, partial: Partial<ReferenceGrantFrom>) => {
    const refs = [...(data.spec?.from || [])]
    refs[index] = { ...refs[index], ...partial }
    onChange({ ...data, spec: { ...data.spec, from: refs } })
  }

  const addFromRef = () => {
    const refs = [...(data.spec?.from || []), { group: 'gateway.networking.k8s.io', kind: '', namespace: '' }]
    onChange({ ...data, spec: { ...data.spec, from: refs } })
  }

  const removeFromRef = (index: number) => {
    const refs = (data.spec?.from || []).filter((_, i) => i !== index)
    onChange({ ...data, spec: { ...data.spec, from: refs } })
  }

  const updateToRef = (index: number, partial: Partial<ReferenceGrantTo>) => {
    const refs = [...(data.spec?.to || [])]
    refs[index] = { ...refs[index], ...partial }
    onChange({ ...data, spec: { ...data.spec, to: refs } })
  }

  const addToRef = () => {
    const refs = [...(data.spec?.to || []), { group: '', kind: '' }]
    onChange({ ...data, spec: { ...data.spec, to: refs } })
  }

  const removeToRef = (index: number) => {
    const refs = (data.spec?.to || []).filter((_, i) => i !== index)
    onChange({ ...data, spec: { ...data.spec, to: refs } })
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

        <Card title={t('section.fromRefs')} size="small">
          {(data.spec?.from || []).map((ref, index) => (
            <Card
              key={index}
              size="small"
              style={{ marginBottom: 8 }}
              extra={
                !readOnly && (data.spec?.from || []).length > 1 ? (
                  <Button
                    type="text"
                    danger
                    icon={<MinusCircleOutlined />}
                    onClick={() => removeFromRef(index)}
                  />
                ) : null
              }
            >
              <Form.Item label={t('field.group')} style={{ marginBottom: 8 }}>
                <Input
                  value={ref.group}
                  onChange={(e) => updateFromRef(index, { group: e.target.value })}
                  placeholder="gateway.networking.k8s.io"
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.kind')} required style={{ marginBottom: 8 }}>
                <Input
                  value={ref.kind}
                  onChange={(e) => updateFromRef(index, { kind: e.target.value })}
                  placeholder="Gateway"
                  disabled={readOnly}
                  style={{ width: 200 }}
                />
              </Form.Item>
              <Form.Item label={t('field.namespace')} required style={{ marginBottom: 0 }}>
                <Input
                  value={ref.namespace}
                  onChange={(e) => updateFromRef(index, { namespace: e.target.value })}
                  placeholder="gateway-system"
                  disabled={readOnly}
                  style={{ width: 240 }}
                />
              </Form.Item>
            </Card>
          ))}
          {!readOnly && (
            <Button type="dashed" icon={<PlusOutlined />} onClick={addFromRef} block>
              {t('btn.addFromRef')}
            </Button>
          )}
        </Card>

        <Card title={t('section.toRefs')} size="small">
          {(data.spec?.to || []).map((ref, index) => (
            <Card
              key={index}
              size="small"
              style={{ marginBottom: 8 }}
              extra={
                !readOnly && (data.spec?.to || []).length > 1 ? (
                  <Button
                    type="text"
                    danger
                    icon={<MinusCircleOutlined />}
                    onClick={() => removeToRef(index)}
                  />
                ) : null
              }
            >
              <Form.Item label={t('field.group')} style={{ marginBottom: 8 }}>
                <Input
                  value={ref.group}
                  onChange={(e) => updateToRef(index, { group: e.target.value })}
                  placeholder='""'
                  disabled={readOnly}
                  style={{ width: 240 }}
                />
              </Form.Item>
              <Form.Item label={t('field.kind')} required style={{ marginBottom: 8 }}>
                <Input
                  value={ref.kind}
                  onChange={(e) => updateToRef(index, { kind: e.target.value })}
                  placeholder="Secret"
                  disabled={readOnly}
                  style={{ width: 200 }}
                />
              </Form.Item>
              <Form.Item label={t('field.name')} style={{ marginBottom: 0 }}>
                <Input
                  value={ref.name || ''}
                  onChange={(e) => updateToRef(index, { name: e.target.value || undefined })}
                  placeholder="my-secret (optional)"
                  disabled={readOnly}
                  style={{ width: 240 }}
                />
              </Form.Item>
            </Card>
          ))}
          {!readOnly && (
            <Button type="dashed" icon={<PlusOutlined />} onClick={addToRef} block>
              {t('btn.addToRef')}
            </Button>
          )}
        </Card>
      </Space>
    </Form>
  )
}

export default ReferenceGrantForm
