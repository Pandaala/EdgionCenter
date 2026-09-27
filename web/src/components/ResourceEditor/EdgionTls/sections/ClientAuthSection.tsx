/**
 * EdgionTls mTLS 客户端认证配置
 */

import React from 'react'
import { Button, Card, Checkbox, Form, Input, InputNumber, Select, Space } from 'antd'
import type { AllowedSan, ClientAuth } from '@/types/edgion-tls'
import { useT } from '@/i18n'

interface ClientAuthSectionProps {
  value?: ClientAuth
  onChange?: (value: ClientAuth | undefined) => void
  disabled?: boolean
}

const ClientAuthSection: React.FC<ClientAuthSectionProps> = ({ value, onChange, disabled = false }) => {
  const t = useT()
  const update = (partial: Partial<ClientAuth>) => onChange?.({ ...value, ...partial })

  const mode = value?.mode || 'Terminate'
  const needsCA = mode === 'Mutual' || mode === 'OptionalMutual'

  return (
    <Card title={t('section.mtls')} size="small">
      <Form.Item label={t('field.authMode')} style={{ marginBottom: 8 }}>
        <Select
          value={mode}
          onChange={(v) => update({ mode: v as any })}
          disabled={disabled}
          style={{ width: 200 }}
        >
          <Select.Option value="Terminate">{t('tls.modeTerminate')}</Select.Option>
          <Select.Option value="Mutual">{t('tls.modeMutual')}</Select.Option>
          <Select.Option value="OptionalMutual">{t('tls.modeOptional')}</Select.Option>
        </Select>
      </Form.Item>

      {needsCA && (
        <>
          <Form.Item label={t('field.caSecretName')} required style={{ marginBottom: 8 }}>
            <Input
              value={value?.caSecretRef?.name || ''}
              onChange={(e) => update({ caSecretRef: { ...value?.caSecretRef, name: e.target.value, namespace: value?.caSecretRef?.namespace } })}
              placeholder="client-ca"
              disabled={disabled}
            />
          </Form.Item>
          <Form.Item label={t('field.caSecretNs')} style={{ marginBottom: 8 }}>
            <Input
              value={value?.caSecretRef?.namespace || ''}
              onChange={(e) => update({ caSecretRef: { ...value?.caSecretRef, name: value?.caSecretRef?.name || '', namespace: e.target.value || undefined } })}
              placeholder="default"
              disabled={disabled}
              style={{ width: 200 }}
            />
          </Form.Item>
          <Form.Item label={t('field.verifyDepth')} style={{ marginBottom: 8 }}>
            <InputNumber
              value={value?.verifyDepth ?? 1}
              onChange={(v) => update({ verifyDepth: v || 1 })}
              min={1} max={9} precision={0}
              disabled={disabled}
              style={{ width: 120 }}
            />
          </Form.Item>
          <Form.Item label="Allowed client SANs">
            {(value?.allowedSans ?? []).map((entry, index) => {
              const patch = (partial: Partial<AllowedSan>) => update({ allowedSans: value!.allowedSans!.map((item, i) => i === index ? { ...item, ...partial } : item) })
              return <Space key={index} wrap style={{ marginBottom: 8 }}>
                <Select value={entry.type} disabled={disabled} options={['DNS', 'URI', 'Email', 'IP', 'OtherName'].map((type) => ({ value: type }))} onChange={(type) => patch({ type, oid: type === 'OtherName' ? entry.oid : undefined })} />
                <Select value={entry.match ?? 'Exact'} disabled={disabled} options={['Exact', 'Prefix', 'Suffix', 'Contains', 'RegularExpression'].map((match) => ({ value: match }))} onChange={(match) => patch({ match })} />
                <Input aria-label={`SAN value ${index + 1}`} value={entry.value} disabled={disabled} onChange={(event) => patch({ value: event.target.value })} />
                <Checkbox checked={entry.ignoreCase ?? false} disabled={disabled} onChange={(event) => patch({ ignoreCase: event.target.checked })}>Ignore case</Checkbox>
                {entry.type === 'OtherName' && <Input aria-label={`SAN OID ${index + 1}`} value={entry.oid} disabled={disabled} placeholder="1.2.3.4" onChange={(event) => patch({ oid: event.target.value })} />}
                {!disabled && <Button onClick={() => update({ allowedSans: value!.allowedSans!.filter((_, i) => i !== index) })}>Remove SAN</Button>}
              </Space>
            })}
            {!disabled && <Button onClick={() => update({ allowedSans: [...(value?.allowedSans ?? []), { type: 'DNS', value: '' }] })}>Add SAN</Button>}
          </Form.Item>
          <Form.Item label="Allowed client common names"><Select mode="tags" value={value?.allowedCns ?? []} disabled={disabled} onChange={(allowedCns) => update({ allowedCns })} style={{ width: '100%' }} /></Form.Item>
        </>
      )}
    </Card>
  )
}

export default ClientAuthSection
