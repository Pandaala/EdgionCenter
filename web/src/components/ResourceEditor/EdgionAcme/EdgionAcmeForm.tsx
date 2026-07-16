import React, { useEffect, useState } from 'react'
import * as yaml from 'js-yaml'
import { Form, Input, InputNumber, Select, Switch, Card, Space, Divider, message } from 'antd'
import MetadataSection from '../common/MetadataSection'
import type { Dns01Challenge, EdgionAcme, Http01Challenge } from '@/types/edgion-acme'
import { replaceChallengeType } from '@/utils/edgionacme'
import { useT } from '@/i18n'

interface EdgionAcmeFormProps {
  data: EdgionAcme
  onChange: (data: EdgionAcme) => void
  readOnly?: boolean
  isCreate?: boolean
  onValidityChange?: (valid: boolean) => void
}

const KEY_TYPE_OPTIONS = ['ecdsa-p256', 'ecdsa-p384'] as const

interface ParentRefsEditorProps {
  value: NonNullable<EdgionAcme['spec']['autoEdgionTls']>['parentRefs']
  onChange: (value: NonNullable<EdgionAcme['spec']['autoEdgionTls']>['parentRefs']) => void
  readOnly: boolean
  onValidityChange: (valid: boolean) => void
}

const ParentRefsEditor: React.FC<ParentRefsEditorProps> = ({ value, onChange, readOnly, onValidityChange }) => {
  const [draft, setDraft] = useState(() => yaml.dump(value ?? [], { lineWidth: -1, noRefs: true }))
  useEffect(() => setDraft(yaml.dump(value ?? [], { lineWidth: -1, noRefs: true })), [value])

  const commit = (nextDraft = draft, report = true) => {
    try {
      const parsed = yaml.load(nextDraft)
      if (!Array.isArray(parsed)) throw new Error('Parent references must be a YAML array')
      onChange(parsed as NonNullable<ParentRefsEditorProps['value']>)
      onValidityChange(true)
    } catch (error) {
      onValidityChange(false)
      if (report) message.error(error instanceof Error ? error.message : 'Invalid parent references YAML')
    }
  }

  return (
    <Input.TextArea
      value={draft}
      onChange={(event) => {
        setDraft(event.target.value)
        commit(event.target.value, false)
      }}
      onBlur={() => commit()}
      rows={6}
      disabled={readOnly}
      style={{ fontFamily: 'monospace' }}
    />
  )
}

const EdgionAcmeForm: React.FC<EdgionAcmeFormProps> = ({ data, onChange, readOnly = false, isCreate = true, onValidityChange = () => undefined }) => {
  const t = useT()

  const spec = data.spec || {}
  const challenge = spec.challenge || { type: 'http-01' }
  const storage = spec.storage || { secretName: '' }
  const renewal = spec.renewal
  const autoTls = spec.autoEdgionTls
  const updateSpec = (partial: Partial<typeof data.spec>) =>
    onChange({ ...data, spec: { ...data.spec, ...partial } })

  const updateChallenge = (partial: Record<string, unknown>) =>
    updateSpec({ challenge: { ...challenge, ...partial } as typeof challenge })

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
          onChange={(metadata) => onChange({ ...data, metadata: { ...data.metadata, ...metadata } })}
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

          <Form.Item label={t('field.privateKeySecretName')} required style={{ marginBottom: 8 }}>
            <Input
              value={spec.privateKeySecretRef?.name ?? ''}
              onChange={(event) => updateSpec({
                privateKeySecretRef: { ...spec.privateKeySecretRef, name: event.target.value },
              })}
              disabled={readOnly}
              placeholder="acme-account"
            />
          </Form.Item>
          <Form.Item label={t('field.privateKeySecretNamespace')} style={{ marginBottom: 8 }}>
            <Input
              value={spec.privateKeySecretRef?.namespace ?? ''}
              onChange={(event) => updateSpec({
                privateKeySecretRef: {
                  ...spec.privateKeySecretRef,
                  name: spec.privateKeySecretRef?.name ?? '',
                  namespace: event.target.value || undefined,
                },
              })}
              disabled={readOnly}
              placeholder="default"
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
              onChange={(val) => updateSpec({ challenge: replaceChallengeType(challenge, val) })}
              disabled={readOnly}
              style={{ width: 160 }}
            >
              <Select.Option value="http-01">http-01</Select.Option>
              <Select.Option value="dns-01">dns-01</Select.Option>
            </Select>
          </Form.Item>

          {challenge.type === 'http-01' && (() => {
            const http01 = challenge as Http01Challenge
            return (
            <>
              <Form.Item label={t('field.gwRefName')} style={{ marginBottom: 8 }}>
                <Input
                  value={http01.gatewayRef?.name || ''}
                  onChange={(e) => updateChallenge({ gatewayRef: { ...http01.gatewayRef, name: e.target.value } })}
                  placeholder="my-gateway"
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.gwRefNs')} style={{ marginBottom: 0 }}>
                <Input
                  value={http01.gatewayRef?.namespace || ''}
                  onChange={(e) => updateChallenge({ gatewayRef: { ...http01.gatewayRef, name: http01.gatewayRef?.name || '', namespace: e.target.value || undefined } })}
                  placeholder="default"
                  disabled={readOnly}
                  style={{ width: 300 }}
                />
              </Form.Item>
            </>
            )
          })()}

          {challenge.type === 'dns-01' && (() => {
            const dns01 = challenge as Dns01Challenge
            return (
            <>
              <Form.Item label={t('field.dnsProvider')} style={{ marginBottom: 8 }}>
                <Input
                  value={dns01.provider || ''}
                  onChange={(e) => updateChallenge({ provider: e.target.value })}
                  placeholder="cloudflare"
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.credRefName')} style={{ marginBottom: 8 }}>
                <Input
                  value={dns01.credentialRef?.name || ''}
                  onChange={(e) => updateChallenge({ credentialRef: { ...dns01.credentialRef, name: e.target.value } })}
                  placeholder="cloudflare-api-token"
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.credRefNs')} style={{ marginBottom: 8 }}>
                <Input
                  value={dns01.credentialRef?.namespace || ''}
                  onChange={(e) => updateChallenge({ credentialRef: { ...dns01.credentialRef, name: dns01.credentialRef?.name || '', namespace: e.target.value || undefined } })}
                  placeholder="default"
                  disabled={readOnly}
                  style={{ width: 300 }}
                />
              </Form.Item>
              <Form.Item label={t('field.propagationTimeout')} style={{ marginBottom: 8 }}>
                <InputNumber
                  value={dns01.propagationTimeout}
                  onChange={(val) => updateChallenge({ propagationTimeout: val ?? undefined })}
                  placeholder="120"
                  disabled={readOnly}
                  min={0}
                  style={{ width: 160 }}
                />
              </Form.Item>
              <Form.Item label={t('field.propagationInterval')} style={{ marginBottom: 0 }}>
                <InputNumber
                  value={dns01.propagationCheckInterval}
                  onChange={(val) => updateChallenge({ propagationCheckInterval: val ?? undefined })}
                  placeholder="15"
                  disabled={readOnly}
                  min={0}
                  style={{ width: 160 }}
                />
              </Form.Item>
            </>
            )
          })()}
        </Card>

        <Card title={t('section.externalAccountBinding')} size="small">
          <Form.Item label={t('field.eabEnabled')} style={{ marginBottom: 8 }}>
            <Switch
              checked={spec.externalAccountBinding !== undefined}
              onChange={(enabled) => updateSpec({
                externalAccountBinding: enabled
                  ? (spec.externalAccountBinding ?? { keyId: '', keySecretRef: { name: '' } })
                  : undefined,
              })}
              disabled={readOnly}
            />
          </Form.Item>
          {spec.externalAccountBinding && (
            <>
              <Form.Item label={t('field.eabKeyId')} required style={{ marginBottom: 8 }}>
                <Input
                  value={spec.externalAccountBinding.keyId}
                  onChange={(event) => updateSpec({ externalAccountBinding: {
                    ...spec.externalAccountBinding!, keyId: event.target.value,
                  } })}
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.eabSecretName')} required style={{ marginBottom: 8 }}>
                <Input
                  value={spec.externalAccountBinding.keySecretRef.name}
                  onChange={(event) => updateSpec({ externalAccountBinding: {
                    ...spec.externalAccountBinding!,
                    keySecretRef: { ...spec.externalAccountBinding!.keySecretRef, name: event.target.value },
                  } })}
                  disabled={readOnly}
                />
              </Form.Item>
              <Form.Item label={t('field.eabSecretNamespace')} style={{ marginBottom: 0 }}>
                <Input
                  value={spec.externalAccountBinding.keySecretRef.namespace ?? ''}
                  onChange={(event) => updateSpec({ externalAccountBinding: {
                    ...spec.externalAccountBinding!,
                    keySecretRef: {
                      ...spec.externalAccountBinding!.keySecretRef,
                      namespace: event.target.value || undefined,
                    },
                  } })}
                  disabled={readOnly}
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
          <Divider style={{ margin: '12px 0' }} />
          <Form.Item label={t('field.autoTlsParentRefs')} style={{ marginBottom: 0 }}>
            <ParentRefsEditor
              value={autoTls?.parentRefs}
              onChange={(parentRefs) => updateAutoTls({ parentRefs })}
              readOnly={readOnly}
              onValidityChange={onValidityChange}
            />
          </Form.Item>
        </Card>
      </Space>
    </Form>
  )
}

export default EdgionAcmeForm
