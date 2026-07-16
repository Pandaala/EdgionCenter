/**
 * BackendTLSPolicy 表单
 */

import React from 'react'
import { Form, Input, Card, Space, Button, Select, Switch } from 'antd'
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
                    data-testid="backendtlspolicy-target-remove"
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
              <Form.Item label="Section name" style={{ marginBottom: 0 }}>
                <Input value={ref.sectionName} onChange={(e) => updateTargetRef(index, { sectionName: e.target.value || undefined })} disabled={readOnly} placeholder="Optional port/listener section" />
              </Form.Item>
            </Card>
          ))}
          {!readOnly && (
            <Button data-testid="backendtlspolicy-target-add" type="dashed" icon={<PlusOutlined />} onClick={addTargetRef} block>
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

          <Form.Item label="Use system CA bundle">
            <Switch checked={data.spec.validation.wellKnownCACertificates === 'System'} disabled={readOnly}
              onChange={(checked) => onChange({ ...data, spec: { ...data.spec, validation: { ...data.spec.validation, wellKnownCACertificates: checked ? 'System' : undefined, caCertificateRefs: checked ? [] : data.spec.validation.caCertificateRefs } } })} />
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
                  <Select
                    value={ref.kind}
                    onChange={(kind) => updateCaRef(index, { kind })}
                    disabled={readOnly}
                    style={{ width: 200 }}
                    options={['Secret', 'ConfigMap'].map((value) => ({ value }))}
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
          <Card title="Subject alternative names" size="small" style={{ marginTop: 12 }}>
            {(data.spec.validation.subjectAltNames || []).map((san, index) => <Space key={index} style={{ display: 'flex', marginBottom: 8 }}>
              <Select value={san.type} disabled={readOnly} style={{ width: 130 }} options={['Hostname','URI'].map(value => ({ value }))} onChange={(type) => { const next=[...(data.spec.validation.subjectAltNames||[])]; next[index]={ type, ...(type==='Hostname'?{hostname:''}:{uri:''}) } as any; onChange({...data,spec:{...data.spec,validation:{...data.spec.validation,subjectAltNames:next}}}) }} />
              <Input disabled={readOnly} value={san.type === 'Hostname' ? san.hostname : san.uri} placeholder={san.type === 'Hostname' ? 'api.internal' : 'spiffe://cluster/service'} onChange={(e) => { const next=[...(data.spec.validation.subjectAltNames||[])]; next[index]=san.type==='Hostname'?{type:'Hostname',hostname:e.target.value}:{type:'URI',uri:e.target.value}; onChange({...data,spec:{...data.spec,validation:{...data.spec.validation,subjectAltNames:next}}}) }} />
              {!readOnly && <Button danger type="text" icon={<MinusCircleOutlined />} onClick={() => onChange({...data,spec:{...data.spec,validation:{...data.spec.validation,subjectAltNames:(data.spec.validation.subjectAltNames||[]).filter((_,i)=>i!==index)}}})} />}
            </Space>)}
            {!readOnly && <Button type="dashed" icon={<PlusOutlined />} onClick={() => onChange({...data,spec:{...data.spec,validation:{...data.spec.validation,subjectAltNames:[...(data.spec.validation.subjectAltNames||[]),{type:'Hostname',hostname:''}]}}})}>Add SAN</Button>}
          </Card>
          <Form.Item label="Client certificate Secret name" extra="Same namespace only. Enter a bare Secret name, never namespace/name." style={{ marginTop: 12 }}>
            <Input disabled={readOnly} value={data.spec.options?.['edgion.io/client-certificate-ref'] || ''} onChange={(e) => onChange({...data,spec:{...data.spec,options:{...data.spec.options,'edgion.io/client-certificate-ref':e.target.value}}})} />
          </Form.Item>
        </Card>
      </Space>
    </Form>
  )
}

export default BackendTLSPolicyForm
