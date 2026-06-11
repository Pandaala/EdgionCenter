/**
 * EdgionGatewayConfig 表单
 * 集群级资源，无 namespace
 */

import React from 'react'
import { Form, Input, InputNumber, Switch, Card, Space, Select } from 'antd'
import type { EdgionGatewayConfig } from '@/types/edgion-gateway-config'
import { useT } from '@/i18n'

interface EdgionGatewayConfigFormProps {
  data: EdgionGatewayConfig
  onChange: (data: EdgionGatewayConfig) => void
  readOnly?: boolean
  isCreate?: boolean
}

const EdgionGatewayConfigForm: React.FC<EdgionGatewayConfigFormProps> = ({
  data,
  onChange,
  readOnly = false,
  isCreate = true,
}) => {
  const t = useT()

  const updateServer = (partial: Partial<NonNullable<EdgionGatewayConfig['spec']['server']>>) =>
    onChange({ ...data, spec: { ...data.spec, server: { ...data.spec?.server, ...partial } } })

  const updateClientTimeout = (partial: Partial<NonNullable<NonNullable<EdgionGatewayConfig['spec']['httpTimeout']>['client']>>) =>
    onChange({
      ...data,
      spec: {
        ...data.spec,
        httpTimeout: {
          ...data.spec?.httpTimeout,
          client: { ...data.spec?.httpTimeout?.client, ...partial },
        },
      },
    })

  const updateBackendTimeout = (partial: Partial<NonNullable<NonNullable<EdgionGatewayConfig['spec']['httpTimeout']>['backend']>>) =>
    onChange({
      ...data,
      spec: {
        ...data.spec,
        httpTimeout: {
          ...data.spec?.httpTimeout,
          backend: { ...data.spec?.httpTimeout?.backend, ...partial },
        },
      },
    })

  const updateRealIp = (partial: Partial<NonNullable<EdgionGatewayConfig['spec']['realIp']>>) =>
    onChange({ ...data, spec: { ...data.spec, realIp: { ...data.spec?.realIp, ...partial } } })

  const updatePreflight = (partial: Partial<NonNullable<EdgionGatewayConfig['spec']['preflightPolicy']>>) =>
    onChange({ ...data, spec: { ...data.spec, preflightPolicy: { ...data.spec?.preflightPolicy, ...partial } } })

  const server = data.spec?.server || {}
  const client = data.spec?.httpTimeout?.client || {}
  const backend = data.spec?.httpTimeout?.backend || {}
  const realIp = data.spec?.realIp || {}
  const preflight = data.spec?.preflightPolicy || {}

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        {/* Name (cluster resource, no namespace) */}
        <Card title={t('section.basicInfo')} size="small">
          <Form.Item label={t('field.name')} required style={{ marginBottom: 0 }}>
            <Input
              value={data.metadata?.name || ''}
              onChange={(e) => onChange({ ...data, metadata: { ...data.metadata, name: e.target.value } })}
              placeholder="default-config"
              disabled={readOnly || !isCreate}
              style={{ width: 320 }}
            />
          </Form.Item>
        </Card>

        {/* Server Config */}
        <Card title={t('section.serverConfig')} size="small">
          <Form.Item label={t('field.gracePeriod')} style={{ marginBottom: 8 }}>
            <InputNumber
              value={server.gracePeriodSeconds}
              onChange={(v) => updateServer({ gracePeriodSeconds: v ?? undefined })}
              placeholder="30"
              min={0}
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.shutdownTimeout')} style={{ marginBottom: 8 }}>
            <InputNumber
              value={server.gracefulShutdownTimeoutS}
              onChange={(v) => updateServer({ gracefulShutdownTimeoutS: v ?? undefined })}
              placeholder="10"
              min={0}
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.keepalivePoolSize')} style={{ marginBottom: 8 }}>
            <InputNumber
              value={server.upstreamKeepalivePoolSize}
              onChange={(v) => updateServer({ upstreamKeepalivePoolSize: v ?? undefined })}
              min={0}
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.enableCompression')} style={{ marginBottom: 0 }}>
            <Switch
              checked={!!server.enableCompression}
              onChange={(checked) => updateServer({ enableCompression: checked })}
              disabled={readOnly}
            />
          </Form.Item>
        </Card>

        {/* HTTP Timeout */}
        <Card title={t('section.httpTimeout')} size="small">
          <Form.Item label={t('field.clientReadTimeout')} style={{ marginBottom: 8 }}>
            <Input
              value={client.readTimeout || ''}
              onChange={(e) => updateClientTimeout({ readTimeout: e.target.value || undefined })}
              placeholder="60s"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.clientWriteTimeout')} style={{ marginBottom: 8 }}>
            <Input
              value={client.writeTimeout || ''}
              onChange={(e) => updateClientTimeout({ writeTimeout: e.target.value || undefined })}
              placeholder="60s"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.clientKeepaliveTimeout')} style={{ marginBottom: 8 }}>
            <Input
              value={client.keepaliveTimeout || ''}
              onChange={(e) => updateClientTimeout({ keepaliveTimeout: e.target.value || undefined })}
              placeholder="120s"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.backendConnectTimeout')} style={{ marginBottom: 8 }}>
            <Input
              value={backend.defaultConnectTimeout || ''}
              onChange={(e) => updateBackendTimeout({ defaultConnectTimeout: e.target.value || undefined })}
              placeholder="5s"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.backendRequestTimeout')} style={{ marginBottom: 8 }}>
            <Input
              value={backend.defaultRequestTimeout || ''}
              onChange={(e) => updateBackendTimeout({ defaultRequestTimeout: e.target.value || undefined })}
              placeholder="60s"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.backendIdleTimeout')} style={{ marginBottom: 0 }}>
            <Input
              value={backend.defaultIdleTimeout || ''}
              onChange={(e) => updateBackendTimeout({ defaultIdleTimeout: e.target.value || undefined })}
              placeholder="120s"
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
        </Card>

        {/* Retry & Resilience */}
        <Card title={t('section.retryResilience')} size="small">
          <Form.Item label={t('field.maxRetries')} style={{ marginBottom: 0 }}>
            <InputNumber
              value={data.spec?.maxRetries}
              onChange={(v) => onChange({ ...data, spec: { ...data.spec, maxRetries: v ?? undefined } })}
              placeholder="3"
              min={0}
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
        </Card>

        {/* Real IP */}
        <Card title={t('section.realIp')} size="small">
          <Form.Item label={t('field.realIpHeader')} style={{ marginBottom: 8 }}>
            <Input
              value={realIp.realIpHeader || ''}
              onChange={(e) => updateRealIp({ realIpHeader: e.target.value || undefined })}
              placeholder="X-Forwarded-For"
              disabled={readOnly}
              style={{ width: 280 }}
            />
          </Form.Item>
          <Form.Item label={t('field.trustedIps')} style={{ marginBottom: 8 }}>
            <Select
              mode="tags"
              value={realIp.trustedIps || []}
              onChange={(v) => updateRealIp({ trustedIps: v.length > 0 ? v : undefined })}
              placeholder="10.0.0.0/8"
              disabled={readOnly}
              style={{ width: '100%' }}
              tokenSeparators={[',']}
            />
          </Form.Item>
          <Form.Item label={t('field.recursive')} style={{ marginBottom: 0 }}>
            <Switch
              checked={!!realIp.recursive}
              onChange={(checked) => updateRealIp({ recursive: checked })}
              disabled={readOnly}
            />
          </Form.Item>
        </Card>

        {/* Preflight Policy */}
        <Card title={t('section.preflightPolicy')} size="small">
          <Form.Item label={t('field.preflightMode')} style={{ marginBottom: 8 }}>
            <Input
              value={preflight.mode || ''}
              onChange={(e) => updatePreflight({ mode: e.target.value || undefined })}
              placeholder="cors-standard"
              disabled={readOnly}
              style={{ width: 240 }}
            />
          </Form.Item>
          <Form.Item label={t('field.preflightStatusCode')} style={{ marginBottom: 0 }}>
            <InputNumber
              value={preflight.statusCode}
              onChange={(v) => updatePreflight({ statusCode: v ?? undefined })}
              placeholder="204"
              min={100}
              max={599}
              disabled={readOnly}
              style={{ width: 160 }}
            />
          </Form.Item>
        </Card>
      </Space>
    </Form>
  )
}

export default EdgionGatewayConfigForm
