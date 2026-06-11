/**
 * EdgionAcme 表单
 */

import React from 'react'
import { Form, Input, InputNumber, Select, Switch, Card, Space } from 'antd'
import MetadataSection from '../common/MetadataSection'
import type { EdgionAcme } from '@/types/edgion-acme'
import { useT } from '@/i18n'

interface EdgionAcmeFormProps {
  data: EdgionAcme
  onChange: (data: EdgionAcme) => void
  readOnly?: boolean
  isCreate?: boolean
}

const KEY_TYPE_OPTIONS = ['RSA2048', 'RSA4096', 'EC256', 'EC384']

const EdgionAcmeForm: React.FC<EdgionAcmeFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()

  const spec = data.spec || {}
  const challenge = spec.challenge || { type: 'http-01' }
  const storage = spec.storage || { secretName: '' }
  const renewal = spec.renewal
  const autoTls = spec.autoEdgionTls
  const http01 = challenge.http01 || {}
  const dns01 = challenge.dns01 || {}

  const updateSpec = (partial: Partial<typeof data.spec>) =>
    onChange({ ...data, spec: { ...data.spec, ...partial } })

  const updateChallenge = (partial: Partial<typeof challenge>) =>
    updateSpec({ challenge: { ...challenge, ...partial } })

  const updateHttp01 = (partial: Partial<typeof http01>) =>
    updateChallenge({ http01: { ...http01, ...partial } })

  const updateDns01 = (partial: Partial<typeof dns01>) =>
    updateChallenge({ dns01: { ...dns01, ...partial } })

  const updateStorage = (partial: Partial<typeof storage>) =>
    updateSpec({ storage: { ...storage, ...partial } })

  const updateRenewal = (partial: Partial<NonNullable<typeof renewal>>) =>
    updateSpec({ renewal: { ...renewal, ...partial } })

  const updateAutoTls = (partial: Partial<NonNullable<typeof autoTls>>) =>
    updateSpec({ autoEdgionTls: { ...autoTls, ...partial } })

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        {/* Basic Config */}
        <Card title={t('section.acmeBasic')} size="small">
          <Form.Item label={t('field.email')} required style={{ marginBottom: 8 }}>
            <Input
              value={spec.email || ''}
              onChange={(e) => updateSpec({ email: e.target.value })}
              placeholder="admin@example.com"
              disabled={readOnly}
            />
          </Form.Item>

          <Form.Item label={t('field.acmeDomains')} style={{ marginBottom: 8 }}>
            <Select
              mode="tags"
              value={spec.domains || []}
              onChange={(val) => updateSpec({ domains: val })}
              placeholder="example.com"
              disabled={readOnly}
              tokenSeparators={[',']}
              style={{ width: '100%' }}
            />
          </Form.Item>

          <Form.Item label={t('field.acmeServer')} style={{ marginBottom: 8 }}>
            <Input
              value={spec.server || ''}
              onChange={(e) => updateSpec({ server: e.target.value || undefined })}
              placeholder="https://acme-v02.api.letsencrypt.org/directory"
              disabled={readOnly}
            />
          </Form.Item>

          <Form.Item label={t('field.keyType')} style={{ marginBottom: 0 }}>
            <Select
              value={spec.keyType}
              onChange={(val) => updateSpec({ keyType: val || undefined })}
              placeholder={t('field.keyType')}
              disabled={readOnly}
              allowClear
              style={{ width: 160 }}
            >
              {KEY_TYPE_OPTIONS.map((k) => (
                <Select.Option key={k} value={k}>{k}</Select.Option>
              ))}
            </Select>
          </Form.Item>
        </Card>

        {/* Challenge Config */}
        <Card title={t('section.challenge')} size="small">
          <Form.Item label={t('field.challengeType')} required style={{ marginBottom: 8 }}>
            <Select
              value={challenge.type || 'http-01'}
              onChange={(val) => updateChallenge({ type: val, http01: val === 'http-01' ? (http01 || {}) : undefined, dns01: val === 'dns-01' ? (dns01 || {}) : undefined })}
              disabled={readOnly}
              style={{ width: 160 }}
            >
              <Select.Option value="http-01">http-01</Select.Option>
              <Select.Option value="dns-01">dns-01</Select.Option>
            </Select>
          </Form.Item>

          {challenge.type === 'http-01' && (
            <>
              <Form.Item label={t('field.gwRefName')} style={{ marginBottom: 8 }}>
                <Input
                  value={http01.gatewayRef?.name || ''}
                  onChange={(e) => updateHttp01({ gatewayRef: { ...http01.gatewayRef, name: e.target.value } })}
                  placeholder="my-gateway"
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.gwRefNs')} style={{ marginBottom: 0 }}>
                <Input
                  value={http01.gatewayRef?.namespace || ''}
                  onChange={(e) => updateHttp01({ gatewayRef: { ...http01.gatewayRef, name: http01.gatewayRef?.name || '', namespace: e.target.value || undefined } })}
                  placeholder="default"
                  disabled={readOnly}
                  style={{ width: 300 }}
                />
              </Form.Item>
            </>
          )}

          {challenge.type === 'dns-01' && (
            <>
              <Form.Item label={t('field.dnsProvider')} style={{ marginBottom: 8 }}>
                <Input
                  value={dns01.provider || ''}
                  onChange={(e) => updateDns01({ provider: e.target.value || undefined })}
                  placeholder="cloudflare"
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.credRefName')} style={{ marginBottom: 8 }}>
                <Input
                  value={dns01.credentialRef?.name || ''}
                  onChange={(e) => updateDns01({ credentialRef: { ...dns01.credentialRef, name: e.target.value } })}
                  placeholder="cloudflare-api-token"
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.credRefNs')} style={{ marginBottom: 8 }}>
                <Input
                  value={dns01.credentialRef?.namespace || ''}
                  onChange={(e) => updateDns01({ credentialRef: { ...dns01.credentialRef, name: dns01.credentialRef?.name || '', namespace: e.target.value || undefined } })}
                  placeholder="default"
                  disabled={readOnly}
                  style={{ width: 300 }}
                />
              </Form.Item>
              <Form.Item label={t('field.propagationTimeout')} style={{ marginBottom: 8 }}>
                <InputNumber
                  value={dns01.propagationTimeout}
                  onChange={(val) => updateDns01({ propagationTimeout: val ?? undefined })}
                  placeholder="120"
                  disabled={readOnly}
                  min={0}
                  style={{ width: 160 }}
                />
              </Form.Item>
              <Form.Item label={t('field.propagationInterval')} style={{ marginBottom: 0 }}>
                <InputNumber
                  value={dns01.propagationCheckInterval}
                  onChange={(val) => updateDns01({ propagationCheckInterval: val ?? undefined })}
                  placeholder="15"
                  disabled={readOnly}
                  min={0}
                  style={{ width: 160 }}
                />
              </Form.Item>
            </>
          )}
        </Card>

        {/* Storage */}
        <Card title={t('section.storage')} size="small">
          <Form.Item label={t('field.storageSecretName')} required style={{ marginBottom: 8 }}>
            <Input
              value={storage.secretName || ''}
              onChange={(e) => updateStorage({ secretName: e.target.value })}
              placeholder="acme-cert"
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item label={t('field.storageSecretNs')} style={{ marginBottom: 0 }}>
            <Input
              value={storage.secretNamespace || ''}
              onChange={(e) => updateStorage({ secretNamespace: e.target.value || undefined })}
              placeholder="default"
              disabled={readOnly}
              style={{ width: 300 }}
            />
          </Form.Item>
        </Card>

        {/* Renewal */}
        <Card title={t('section.renewal')} size="small">
          <Form.Item label={t('field.renewBeforeDays')} style={{ marginBottom: 8 }}>
            <InputNumber
              value={renewal?.renewBeforeDays ?? 30}
              onChange={(val) => updateRenewal({ renewBeforeDays: val ?? undefined })}
              disabled={readOnly}
              min={1}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.checkInterval')} style={{ marginBottom: 8 }}>
            <InputNumber
              value={renewal?.checkInterval}
              onChange={(val) => updateRenewal({ checkInterval: val ?? undefined })}
              placeholder="3600"
              disabled={readOnly}
              min={0}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.failBackoff')} style={{ marginBottom: 0 }}>
            <InputNumber
              value={renewal?.failBackoff}
              onChange={(val) => updateRenewal({ failBackoff: val ?? undefined })}
              placeholder="300"
              disabled={readOnly}
              min={0}
              style={{ width: 160 }}
            />
          </Form.Item>
        </Card>

        {/* Auto EdgionTls */}
        <Card title={t('section.autoTls')} size="small">
          <Form.Item label={t('field.autoTlsEnabled')} style={{ marginBottom: 8 }}>
            <Switch
              checked={autoTls?.enabled ?? true}
              onChange={(val) => updateAutoTls({ enabled: val })}
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item label={t('field.autoTlsName')} style={{ marginBottom: 0 }}>
            <Input
              value={autoTls?.name || ''}
              onChange={(e) => updateAutoTls({ name: e.target.value || undefined })}
              placeholder=""
              disabled={readOnly}
              style={{ width: 300 }}
            />
          </Form.Item>
        </Card>
      </Space>
    </Form>
  )
}

export default EdgionAcmeForm
