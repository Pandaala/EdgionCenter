/**
 * TCP/TLS Route 专用 Annotations 编辑器
 * 管理 edgion.io/edgion-stream-plugins, edgion.io/proxy-protocol, edgion.io/max-connect-retries
 */

import React from 'react'
import { Card, Form, Input, Select } from 'antd'
import { useT } from '@/i18n'

interface StreamAnnotationsSectionProps {
  kind: 'TCPRoute' | 'UDPRoute' | 'TLSRoute'
  annotations: Record<string, string>
  onChange: (annotations: Record<string, string>) => void
  disabled?: boolean
}

const STREAM_PLUGINS_KEY = 'edgion.io/edgion-stream-plugins'
const PROXY_PROTOCOL_KEY = 'edgion.io/proxy-protocol'
const MAX_RETRIES_KEY = 'edgion.io/max-connect-retries'
const KEEPALIVE_TIME_KEY = 'edgion.io/tcp-keepalive-time'
const KEEPALIVE_INTERVAL_KEY = 'edgion.io/tcp-keepalive-interval'
const KEEPALIVE_PROBES_KEY = 'edgion.io/tcp-keepalive-probes'

const StreamAnnotationsSection: React.FC<StreamAnnotationsSectionProps> = ({
  annotations,
  kind,
  onChange,
  disabled = false,
}) => {
  const t = useT()

  const update = (key: string, value: string) => {
    const next = { ...annotations }
    if (value === '' || value === undefined) {
      delete next[key]
    } else {
      next[key] = value
    }
    onChange(next)
  }

  return (
    <Card title={t('section.edgionExt')} size="small">
      <Form.Item
        label={t('field.streamPluginsRef')}
        help={t('field.streamPluginsHelp')}
        style={{ marginBottom: 12 }}
      >
        <Input
          value={annotations[STREAM_PLUGINS_KEY] || ''}
          onChange={(e) => update(STREAM_PLUGINS_KEY, e.target.value)}
          placeholder="default/my-stream-plugins"
          disabled={disabled}
          allowClear
        />
      </Form.Item>

      {kind === 'TLSRoute' && <Form.Item
        label={t('field.proxyProtocol')}
        style={{ marginBottom: 12 }}
      >
        <Select
          value={annotations[PROXY_PROTOCOL_KEY] || undefined}
          onChange={(v) => update(PROXY_PROTOCOL_KEY, v || '')}
          placeholder={t('stream.noEnable')}
          disabled={disabled}
          allowClear
          style={{ width: 160 }}
        >
          <Select.Option value="v2">v2</Select.Option>
        </Select>
      </Form.Item>}

      {kind === 'TLSRoute' && <Form.Item
        label={t('field.maxConnRetries')}
        style={{ marginBottom: 0 }}
      >
        <Input
          value={annotations[MAX_RETRIES_KEY] || ''}
          onChange={(e) => update(MAX_RETRIES_KEY, e.target.value)}
          placeholder="3"
          disabled={disabled}
          style={{ width: 160 }}
          type="number"
          min={1}
          max={5}
          allowClear
        />
      </Form.Item>}

      {kind !== 'UDPRoute' && <>
        <Form.Item label={t('field.tcpKeepaliveTime')} style={{ marginBottom: 12 }}>
          <Input value={annotations[KEEPALIVE_TIME_KEY] || ''} onChange={(e) => update(KEEPALIVE_TIME_KEY, e.target.value)} disabled={disabled} type="number" min={1} max={7200} allowClear />
        </Form.Item>
        <Form.Item label={t('field.tcpKeepaliveInterval')} style={{ marginBottom: 12 }}>
          <Input value={annotations[KEEPALIVE_INTERVAL_KEY] || ''} onChange={(e) => update(KEEPALIVE_INTERVAL_KEY, e.target.value)} disabled={disabled} type="number" min={1} max={75} allowClear />
        </Form.Item>
        <Form.Item label={t('field.tcpKeepaliveProbes')} style={{ marginBottom: 0 }}>
          <Input value={annotations[KEEPALIVE_PROBES_KEY] || ''} onChange={(e) => update(KEEPALIVE_PROBES_KEY, e.target.value)} disabled={disabled} type="number" min={1} max={20} allowClear />
        </Form.Item>
      </>}
    </Card>
  )
}

export default StreamAnnotationsSection
