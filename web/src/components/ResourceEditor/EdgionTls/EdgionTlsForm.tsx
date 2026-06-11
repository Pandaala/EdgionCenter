/**
 * EdgionTls 表单
 */

import React from 'react'
import { Form, Input, Card, Select, Space } from 'antd'
import MetadataSection from '../common/MetadataSection'
import HostnamesSection from '../common/HostnamesSection'
import ClientAuthSection from './sections/ClientAuthSection'
import type { EdgionTls } from '@/types/edgion-tls'
import { useT } from '@/i18n'

interface EdgionTlsFormProps {
  data: EdgionTls
  onChange: (data: EdgionTls) => void
  readOnly?: boolean
  isCreate?: boolean
}

const TLS_VERSIONS = ['TLS1_0', 'TLS1_1', 'TLS1_2', 'TLS1_3']

const EdgionTlsForm: React.FC<EdgionTlsFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()

  const updateSpec = (partial: Partial<typeof data.spec>) =>
    onChange({ ...data, spec: { ...data.spec, ...partial } })

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        <Card title={t('section.hostnames')} size="small">
          <HostnamesSection
            value={data.spec?.hosts || []}
            onChange={(hosts) => updateSpec({ hosts })}
            disabled={readOnly}
          />
        </Card>

        <Card title={t('section.serverCert')} size="small">
          <Form.Item label={t('field.secretName')} required style={{ marginBottom: 8 }}>
            <Input
              value={data.spec?.secretRef?.name || ''}
              onChange={(e) => updateSpec({ secretRef: { ...data.spec.secretRef, name: e.target.value } })}
              placeholder="my-tls-cert"
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item label={t('field.secretNs')} style={{ marginBottom: 0 }}>
            <Input
              value={data.spec?.secretRef?.namespace || ''}
              onChange={(e) => updateSpec({
                secretRef: { ...data.spec.secretRef, namespace: e.target.value || undefined }
              })}
              placeholder="default"
              disabled={readOnly}
              style={{ width: 200 }}
            />
          </Form.Item>
        </Card>

        <ClientAuthSection
          value={data.spec?.clientAuth}
          onChange={(clientAuth) => updateSpec({ clientAuth })}
          disabled={readOnly}
        />

        <Card title={t('section.tlsConfig')} size="small">
          <Form.Item label={t('field.minTlsVersion')} style={{ marginBottom: 8 }}>
            <Select
              value={data.spec?.minTlsVersion}
              onChange={(v) => updateSpec({ minTlsVersion: v })}
              disabled={readOnly}
              style={{ width: 160 }}
              allowClear
              placeholder={t('field.default')}
            >
              {TLS_VERSIONS.map((v) => <Select.Option key={v} value={v}>{v}</Select.Option>)}
            </Select>
          </Form.Item>
          <Form.Item label={t('field.cipherSuites')} style={{ marginBottom: 0 }}>
            <Select
              mode="tags"
              value={data.spec?.cipherSuites || []}
              onChange={(v) => updateSpec({ cipherSuites: v })}
              disabled={readOnly}
              placeholder={t('ph.cipherSuites')}
              style={{ width: '100%' }}
            />
          </Form.Item>
        </Card>
      </Space>
    </Form>
  )
}

export default EdgionTlsForm
