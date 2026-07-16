/**
 * Gateway 表单
 */

import React from 'react'
import { Form, Input, Card, Space, Button, AutoComplete } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import MetadataSection from '../common/MetadataSection'
import ListenersSection from './sections/ListenersSection'
import type { Gateway } from '@/types/gateway-api/gateway'
import { useT } from '@/i18n'
import GatewayTLSSection from './sections/GatewayTLSSection'

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

        <Card title={t('section.addresses')} size="small">
          {(data.spec?.addresses || []).map((address, index) => (
            <Space key={index} wrap style={{ marginBottom: 8 }}>
              <AutoComplete
                value={address.type || 'IPAddress'}
                onChange={(type) => updateSpec({ addresses: (data.spec.addresses || []).map((item, itemIndex) => itemIndex === index ? { ...item, type } : item) })}
                disabled={readOnly}
                style={{ width: 140 }}
                options={[{ value: 'IPAddress' }, { value: 'Hostname' }, { value: 'NamedAddress' }]}
              />
              <Input
                aria-label={t('field.addressValue')}
                value={address.value}
                onChange={(event) => updateSpec({ addresses: (data.spec.addresses || []).map((item, itemIndex) => itemIndex === index ? { ...item, value: event.target.value } : item) })}
                disabled={readOnly}
                placeholder="10.0.0.1"
                style={{ width: 280 }}
              />
              {!readOnly && <Button data-testid="gateway-address-remove" danger type="text" icon={<MinusCircleOutlined />} aria-label={t('btn.deleteAddress')} onClick={() => updateSpec({ addresses: (data.spec.addresses || []).filter((_, itemIndex) => itemIndex !== index) })} />}
            </Space>
          ))}
          {!readOnly && <Button data-testid="gateway-address-add" type="dashed" block icon={<PlusOutlined />} onClick={() => updateSpec({ addresses: [...(data.spec.addresses || []), { type: 'IPAddress', value: '' }] })}>{t('btn.addAddress')}</Button>}
        </Card>

        <GatewayTLSSection value={data.spec?.tls} onChange={(tls) => updateSpec({ tls })} disabled={readOnly} />

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
