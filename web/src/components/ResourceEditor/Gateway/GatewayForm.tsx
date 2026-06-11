/**
 * Gateway 表单
 */

import React from 'react'
import { Form, Input, Card, Space } from 'antd'
import MetadataSection from '../common/MetadataSection'
import ListenersSection from './sections/ListenersSection'
import type { Gateway } from '@/types/gateway-api/gateway'
import { useT } from '@/i18n'

interface GatewayFormProps {
  data: Gateway
  onChange: (data: Gateway) => void
  readOnly?: boolean
  isCreate?: boolean
}


const GatewayForm: React.FC<GatewayFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()

  const updateSpec = (partial: Partial<typeof data.spec>) =>
    onChange({ ...data, spec: { ...data.spec, ...partial } })

  const annotations = data.metadata?.annotations || {}
  const updateAnnotation = (key: string, value: string) => {
    const next = { ...annotations }
    if (!value) delete next[key]
    else next[key] = value
    onChange({ ...data, metadata: { ...data.metadata, annotations: next } })
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

        <Card title={t('section.gatewayClass')} size="small">
          <Form.Item label={t('field.gwClassName')} required style={{ marginBottom: 0 }}>
            <Input
              value={data.spec?.gatewayClassName || ''}
              onChange={(e) => updateSpec({ gatewayClassName: e.target.value })}
              placeholder="edgion"
              disabled={readOnly}
              style={{ width: 240 }}
            />
          </Form.Item>
        </Card>

        <Card title={t('section.listeners')} size="small">
          <ListenersSection
            value={data.spec?.listeners || []}
            onChange={(listeners) => updateSpec({ listeners })}
            disabled={readOnly}
          />
        </Card>

        <Card title={t('section.edgionExt')} size="small">
          <Form.Item label={t('field.httpsRedirect')} style={{ marginBottom: 8 }}>
            <Input
              value={annotations['edgion.io/http-to-https-redirect'] || ''}
              onChange={(e) => updateAnnotation('edgion.io/http-to-https-redirect', e.target.value)}
              placeholder="true / false"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.httpsRedirectPort')} style={{ marginBottom: 8 }}>
            <Input
              value={annotations['edgion.io/https-redirect-port'] || ''}
              onChange={(e) => updateAnnotation('edgion.io/https-redirect-port', e.target.value)}
              placeholder="443"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.http2')} style={{ marginBottom: 8 }}>
            <Input
              value={annotations['edgion.io/enable-http2'] || ''}
              onChange={(e) => updateAnnotation('edgion.io/enable-http2', e.target.value)}
              placeholder="true / false"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.streamPluginsRef')} style={{ marginBottom: 0 }}>
            <Input
              value={annotations['edgion.io/edgion-stream-plugins'] || ''}
              onChange={(e) => updateAnnotation('edgion.io/edgion-stream-plugins', e.target.value)}
              placeholder="namespace/plugin-name"
              disabled={readOnly}
            />
          </Form.Item>
        </Card>
      </Space>
    </Form>
  )
}

export default GatewayForm
