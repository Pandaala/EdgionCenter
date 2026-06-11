/**
 * EdgionTls mTLS 客户端认证配置
 */

import React from 'react'
import { Card, Form, Input, InputNumber, Select } from 'antd'
import type { ClientAuth } from '@/types/edgion-tls'
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
              onChange={(e) => update({ caSecretRef: { name: e.target.value, namespace: value?.caSecretRef?.namespace } })}
              placeholder="client-ca"
              disabled={disabled}
            />
          </Form.Item>
          <Form.Item label={t('field.caSecretNs')} style={{ marginBottom: 8 }}>
            <Input
              value={value?.caSecretRef?.namespace || ''}
              onChange={(e) => update({ caSecretRef: { name: value?.caSecretRef?.name || '', namespace: e.target.value || undefined } })}
              placeholder="default"
              disabled={disabled}
              style={{ width: 200 }}
            />
          </Form.Item>
          <Form.Item label={t('field.verifyDepth')} style={{ marginBottom: 8 }}>
            <InputNumber
              value={value?.verifyDepth ?? 1}
              onChange={(v) => update({ verifyDepth: v || 1 })}
              min={1} max={9}
              disabled={disabled}
              style={{ width: 120 }}
            />
          </Form.Item>
        </>
      )}
    </Card>
  )
}

export default ClientAuthSection
