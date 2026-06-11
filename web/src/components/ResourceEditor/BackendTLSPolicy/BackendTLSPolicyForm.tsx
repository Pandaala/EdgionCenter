/**
 * BackendTLSPolicy 表单
 */

import React from 'react'
import { Form, Input, Card, Space, Button } from 'antd'
import { PlusOutlined, MinusCircleOutlined } from '@ant-design/icons'
import MetadataSection from '../common/MetadataSection'
import type { BackendTLSPolicy, BackendTLSPolicyTargetRef, BackendTLSPolicyCACertRef } from '@/utils/backendtlspolicy'
import { useT } from '@/i18n'

interface BackendTLSPolicyFormProps {
  data: BackendTLSPolicy
  onChange: (data: BackendTLSPolicy) => void
  readOnly?: boolean
  isCreate?: boolean
}

const BackendTLSPolicyForm: React.FC<BackendTLSPolicyFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()

  const updateTargetRef = (index: number, partial: Partial<BackendTLSPolicyTargetRef>) => {
    const refs = [...(data.spec?.targetRefs || [])]
    refs[index] = { ...refs[index], ...partial }
    onChange({ ...data, spec: { ...data.spec, targetRefs: refs } })
  }

  const addTargetRef = () => {
    const refs = [...(data.spec?.targetRefs || []), { group: '', kind: 'Service', name: '' }]
    onChange({ ...data, spec: { ...data.spec, targetRefs: refs } })
  }

  const removeTargetRef = (index: number) => {
    const refs = (data.spec?.targetRefs || []).filter((_, i) => i !== index)
    onChange({ ...data, spec: { ...data.spec, targetRefs: refs } })
  }

  const updateCaRef = (index: number, partial: Partial<BackendTLSPolicyCACertRef>) => {
    const refs = [...(data.spec?.validation?.caCertificateRefs || [])]
    refs[index] = { ...refs[index], ...partial }
    onChange({ ...data, spec: { ...data.spec, validation: { ...data.spec.validation, caCertificateRefs: refs } } })
  }

  const addCaRef = () => {
    const refs = [...(data.spec?.validation?.caCertificateRefs || []), { name: '', group: '', kind: 'Secret' }]
    onChange({ ...data, spec: { ...data.spec, validation: { ...data.spec.validation, caCertificateRefs: refs } } })
  }

  const removeCaRef = (index: number) => {
    const refs = (data.spec?.validation?.caCertificateRefs || []).filter((_, i) => i !== index)
    onChange({ ...data, spec: { ...data.spec, validation: { ...data.spec.validation, caCertificateRefs: refs } } })
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

        <Card title={t('section.targetRefs')} size="small">
          {(data.spec?.targetRefs || []).map((ref, index) => (
            <Card
              key={index}
              size="small"
              style={{ marginBottom: 8 }}
              extra={
                !readOnly && (data.spec?.targetRefs || []).length > 1 ? (
                  <Button
                    type="text"
                    danger
                    icon={<MinusCircleOutlined />}
                    onClick={() => removeTargetRef(index)}
                  />
                ) : null
              }
            >
              <Form.Item label={t('field.serviceName')} required style={{ marginBottom: 8 }}>
                <Input
                  value={ref.name}
                  onChange={(e) => updateTargetRef(index, { name: e.target.value })}
                  placeholder="my-backend-service"
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.group')} style={{ marginBottom: 8 }}>
                <Input
                  value={ref.group}
                  onChange={(e) => updateTargetRef(index, { group: e.target.value })}
                  placeholder='""'
                  disabled={readOnly}
                  style={{ width: 240 }}
                />
              </Form.Item>
              <Form.Item label={t('field.kind')} style={{ marginBottom: 0 }}>
                <Input
                  value={ref.kind}
                  onChange={(e) => updateTargetRef(index, { kind: e.target.value })}
                  placeholder="Service"
                  disabled={readOnly}
                  style={{ width: 200 }}
                />
              </Form.Item>
            </Card>
          ))}
          {!readOnly && (
            <Button type="dashed" icon={<PlusOutlined />} onClick={addTargetRef} block>
              {t('btn.addTargetRef')}
            </Button>
          )}
        </Card>

        <Card title={t('section.validation')} size="small">
          <Form.Item label={t('field.validationHostname')} required style={{ marginBottom: 12 }}>
            <Input
              value={data.spec?.validation?.hostname || ''}
              onChange={(e) =>
                onChange({ ...data, spec: { ...data.spec, validation: { ...data.spec.validation, hostname: e.target.value } } })
              }
              placeholder="backend.internal"
              disabled={readOnly}
            />
          </Form.Item>

          <Form.Item label={t('field.caRefs')} style={{ marginBottom: 0 }}>
            {(data.spec?.validation?.caCertificateRefs || []).map((ref, index) => (
              <Card
                key={index}
                size="small"
                style={{ marginBottom: 8 }}
                extra={
                  !readOnly && (data.spec?.validation?.caCertificateRefs || []).length > 1 ? (
                    <Button
                      type="text"
                      danger
                      icon={<MinusCircleOutlined />}
                      onClick={() => removeCaRef(index)}
                    />
                  ) : null
                }
              >
                <Form.Item label={t('field.secretName')} required style={{ marginBottom: 8 }}>
                  <Input
                    value={ref.name}
                    onChange={(e) => updateCaRef(index, { name: e.target.value })}
                    placeholder="backend-ca"
                    disabled={readOnly}
                  />
                </Form.Item>
                <Form.Item label={t('field.group')} style={{ marginBottom: 8 }}>
                  <Input
                    value={ref.group}
                    onChange={(e) => updateCaRef(index, { group: e.target.value })}
                    placeholder='""'
                    disabled={readOnly}
                    style={{ width: 240 }}
                  />
                </Form.Item>
                <Form.Item label={t('field.kind')} style={{ marginBottom: 0 }}>
                  <Input
                    value={ref.kind}
                    onChange={(e) => updateCaRef(index, { kind: e.target.value })}
                    placeholder="Secret"
                    disabled={readOnly}
                    style={{ width: 200 }}
                  />
                </Form.Item>
              </Card>
            ))}
            {!readOnly && (
              <Button type="dashed" icon={<PlusOutlined />} onClick={addCaRef} block>
                {t('btn.addCaRef')}
              </Button>
            )}
          </Form.Item>
        </Card>
      </Space>
    </Form>
  )
}

export default BackendTLSPolicyForm
