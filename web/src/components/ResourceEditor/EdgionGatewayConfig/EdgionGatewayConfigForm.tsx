/**
 * EdgionGatewayConfig 表单
 * 集群级资源，无 namespace
 */

import React from 'react'
import { Form, Input, InputNumber, Switch, Card, Space, Select, Button } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import type { EdgionGatewayConfig, IpGroup, ObjectReference, SubjectAltName } from '@/types/edgion-gateway-config'
import { useT } from '@/i18n'

interface EdgionGatewayConfigFormProps {
  data: EdgionGatewayConfig
  onChange: (data: EdgionGatewayConfig) => void
  readOnly?: boolean
  isCreate?: boolean
}

interface ListProps<T> { value: T[]; onChange: (value: T[]) => void; disabled: boolean; maxItems?: number }

const IpGroupsEditor: React.FC<ListProps<IpGroup>> = ({ value, onChange, disabled }) => {
  const t = useT()
  return <Space direction="vertical" style={{ width: '100%' }}>
    {value.map((group, index) => <Card key={index} size="small" type="inner" title={t('egc.ipGroupTitle', { n: index + 1 })} extra={!disabled && <Button data-testid="edgiongatewayconfig-ip-group-remove" danger type="text" icon={<MinusCircleOutlined />} aria-label={t('btn.deleteIpGroup')} onClick={() => onChange(value.filter((_, itemIndex) => itemIndex !== index))} />}>
      <Space wrap>
        <Form.Item label={t('field.name')} required style={{ marginBottom: 0 }}><Input value={group.name} disabled={disabled} onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item))} /></Form.Item>
        <Form.Item label={t('field.descriptionOpt')} style={{ marginBottom: 0 }}><Input value={group.description || ''} disabled={disabled} onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, description: event.target.value || undefined } : item))} /></Form.Item>
      </Space>
      <Form.Item label={t('field.cidrs')} required style={{ marginBottom: 0, marginTop: 8 }}><Select mode="tags" value={group.cidrs} tokenSeparators={[',']} disabled={disabled} onChange={(cidrs) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, cidrs } : item))} /></Form.Item>
    </Card>)}
    {!disabled && <Button data-testid="edgiongatewayconfig-ip-group-add" type="dashed" block icon={<PlusOutlined />} onClick={() => onChange([...value, { name: '', cidrs: [] }])}>{t('btn.addIpGroup')}</Button>}
  </Space>
}

const ObjectRefsEditor: React.FC<ListProps<ObjectReference>> = ({ value, onChange, disabled, maxItems }) => {
  const t = useT()
  return <Space direction="vertical" style={{ width: '100%' }}>
    {value.map((ref, index) => <Space key={index} wrap>
      <Input aria-label={t('field.name')} value={ref.name} disabled={disabled} placeholder="name" onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item))} />
      <Input aria-label={t('field.namespaceOpt')} value={ref.namespace || ''} disabled={disabled} placeholder="namespace" onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, namespace: event.target.value || undefined } : item))} />
      <Input aria-label={t('field.group')} value={ref.group || ''} disabled={disabled} placeholder="group" onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, group: event.target.value } : item))} />
      <Input aria-label={t('field.kind')} value={ref.kind || ''} disabled={disabled} placeholder="Secret" onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, kind: event.target.value || undefined } : item))} />
      {!disabled && <Button danger type="text" icon={<MinusCircleOutlined />} aria-label={t('btn.deleteReference')} onClick={() => onChange(value.filter((_, itemIndex) => itemIndex !== index))} />}
    </Space>)}
    {!disabled && (maxItems === undefined || value.length < maxItems) && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => onChange([...value, { name: '', group: '', kind: 'Secret' }])}>{t('btn.addReference')}</Button>}
  </Space>
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

  const updateSpecBlock = <K extends keyof EdgionGatewayConfig['spec']>(key: K, partial: Record<string, unknown>) =>
    onChange({ ...data, spec: { ...data.spec, [key]: { ...((data.spec[key] || {}) as object), ...partial } } })

  const server = data.spec?.server || {}
  const client = data.spec?.httpTimeout?.client || {}
  const backend = data.spec?.httpTimeout?.backend || {}
  const realIp = data.spec?.realIp || {}
  const preflight = data.spec?.preflightPolicy || {}
  const security = data.spec?.securityProtect || {}
  const tcpTimeout = data.spec?.tcpTimeout || {}
  const loadBalancing = data.spec?.loadBalancing || {}
  const linkSys = data.spec?.linkSys || {}
  const outboundTls = data.spec?.outboundTls || {}
  const outboundValidation = outboundTls.validation || {}
  const dnsResolver = data.spec?.dnsResolver || {}
  const dnsResolverMode = dnsResolver.linkSysRef ? 'linkSysRef' : dnsResolver.servers ? 'servers' : undefined

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
          <Form.Item label={t('field.threads')} style={{ marginBottom: 8 }}><InputNumber value={server.threads} onChange={(v) => updateServer({ threads: v ?? undefined })} min={0} disabled={readOnly} style={{ width: 160 }} /></Form.Item>
          <Form.Item label={t('field.workStealing')} style={{ marginBottom: 8 }}><Switch checked={server.workStealing ?? true} onChange={(workStealing) => updateServer({ workStealing })} disabled={readOnly} /></Form.Item>
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
          <Form.Item label={t('field.downstreamKeepaliveLimit')} style={{ marginBottom: 8, marginTop: 8 }}><InputNumber value={server.downstreamKeepaliveRequestLimit} onChange={(v) => updateServer({ downstreamKeepaliveRequestLimit: v ?? undefined })} min={0} disabled={readOnly} style={{ width: 160 }} /></Form.Item>
          <Form.Item label={t('field.errorLog')} style={{ marginBottom: 0 }}><Input value={server.errorLog || ''} onChange={(event) => updateServer({ errorLog: event.target.value || undefined })} disabled={readOnly} /></Form.Item>
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
          <Form.Item label={t('field.tcpIdleTimeout')} style={{ marginBottom: 8, marginTop: 8 }}><Input value={tcpTimeout.idleTimeout || ''} onChange={(event) => updateSpecBlock('tcpTimeout', { idleTimeout: event.target.value || undefined })} disabled={readOnly} style={{ width: 160 }} /></Form.Item>
          <Form.Item label={t('field.tcpConnectTimeout')} style={{ marginBottom: 8 }}><Input value={tcpTimeout.connectTimeout || ''} onChange={(event) => updateSpecBlock('tcpTimeout', { connectTimeout: event.target.value || undefined })} disabled={readOnly} style={{ width: 160 }} /></Form.Item>
          <Form.Item label={t('field.panicThreshold')} style={{ marginBottom: 0 }}><InputNumber value={loadBalancing.panicThreshold} min={0} max={100} onChange={(value) => updateSpecBlock('loadBalancing', { panicThreshold: value ?? undefined })} disabled={readOnly} /></Form.Item>
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
          <Form.Item label={t('field.trustedIps')} style={{ marginBottom: 8 }}><IpGroupsEditor value={realIp.trustedIps || []} onChange={(trustedIps) => updateRealIp({ trustedIps })} disabled={readOnly} /></Form.Item>
          <Form.Item label={t('field.recursive')} style={{ marginBottom: 0 }}>
            <Switch
              checked={!!realIp.recursive}
              onChange={(checked) => updateRealIp({ recursive: checked })}
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item label={t('field.maxTrustedHops')} style={{ marginBottom: 0, marginTop: 8 }}><InputNumber value={realIp.maxTrustedHops} min={0} onChange={(maxTrustedHops) => updateRealIp({ maxTrustedHops: maxTrustedHops ?? undefined })} disabled={readOnly} /></Form.Item>
        </Card>

        <Card title={t('section.securityProtect')} size="small">
          <Form.Item label={t('field.xForwardedForLimit')} style={{ marginBottom: 8 }}><InputNumber value={security.xForwardedForLimit} min={0} disabled={readOnly} onChange={(value) => updateSpecBlock('securityProtect', { xForwardedForLimit: value ?? undefined })} /></Form.Item>
          <Form.Item label={t('field.requireSniHostMatch')} style={{ marginBottom: 8 }}><Switch checked={security.requireSniHostMatch ?? true} disabled={readOnly} onChange={(value) => updateSpecBlock('securityProtect', { requireSniHostMatch: value })} /></Form.Item>
          <Form.Item label={t('field.fallbackSni')} style={{ marginBottom: 8 }}><Input value={security.fallbackSni || ''} disabled={readOnly} onChange={(event) => updateSpecBlock('securityProtect', { fallbackSni: event.target.value || undefined })} /></Form.Item>
          <Form.Item label={t('field.tlsProxyLogRecord')} style={{ marginBottom: 8 }}><Switch checked={security.tlsProxyLogRecord ?? true} disabled={readOnly} onChange={(value) => updateSpecBlock('securityProtect', { tlsProxyLogRecord: value })} /></Form.Item>
          <Form.Item label={t('field.allowLoopbackUpstream')} style={{ marginBottom: 8 }}><Switch checked={security.allowLoopbackUpstream ?? false} disabled={readOnly} onChange={(value) => updateSpecBlock('securityProtect', { allowLoopbackUpstream: value })} /></Form.Item>
          <Form.Item label={t('field.rejectDuplicateHost')} style={{ marginBottom: 0 }}><Switch checked={security.rejectDuplicateHost ?? true} disabled={readOnly} onChange={(value) => updateSpecBlock('securityProtect', { rejectDuplicateHost: value })} /></Form.Item>
        </Card>

        <Card title={t('section.globalPlugins')} size="small">
          {(data.spec.globalPluginsRef || []).map((ref, index) => <Space key={index} wrap style={{ marginBottom: 8 }}>
            <Input aria-label={t('field.name')} value={ref.name} disabled={readOnly} onChange={(event) => onChange({ ...data, spec: { ...data.spec, globalPluginsRef: (data.spec.globalPluginsRef || []).map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item) } })} />
            <Input aria-label={t('field.namespaceOpt')} value={ref.namespace || ''} disabled={readOnly} onChange={(event) => onChange({ ...data, spec: { ...data.spec, globalPluginsRef: (data.spec.globalPluginsRef || []).map((item, itemIndex) => itemIndex === index ? { ...item, namespace: event.target.value || undefined } : item) } })} />
            {!readOnly && <Button danger type="text" icon={<MinusCircleOutlined />} aria-label={t('btn.deletePluginRef')} onClick={() => onChange({ ...data, spec: { ...data.spec, globalPluginsRef: (data.spec.globalPluginsRef || []).filter((_, itemIndex) => itemIndex !== index) } })} />}
          </Space>)}
          {!readOnly && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => onChange({ ...data, spec: { ...data.spec, globalPluginsRef: [...(data.spec.globalPluginsRef || []), { name: '', namespace: 'default' }] } })}>{t('btn.addPluginRef')}</Button>}
        </Card>

        {/* Preflight Policy */}
        <Card title={t('section.preflightPolicy')} size="small">
          <Form.Item label={t('field.preflightMode')} style={{ marginBottom: 8 }}>
            <Select
              value={preflight.mode}
              onChange={(mode: 'cors-standard' | 'all-options' | undefined) => updatePreflight({ mode })}
              options={[{ value: 'cors-standard' }, { value: 'all-options' }]}
              allowClear
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

        <Card title={t('section.validation')} size="small">
          <Form.Item label={t('field.referenceGrantValidation')} style={{ marginBottom: 0 }}><Switch checked={data.spec.enableReferenceGrantValidation ?? false} disabled={readOnly} onChange={(enableReferenceGrantValidation) => onChange({ ...data, spec: { ...data.spec, enableReferenceGrantValidation } })} /></Form.Item>
        </Card>

        <Card title={t('section.linkSys')} size="small"><Form.Item label={t('field.webhookMaxResponseBytes')} style={{ marginBottom: 0 }}><InputNumber value={linkSys.webhookMaxResponseBytes} min={1} disabled={readOnly} onChange={(value) => updateSpecBlock('linkSys', { webhookMaxResponseBytes: value ?? undefined })} /></Form.Item></Card>

        <Card title={t('section.outboundTls')} size="small">
          <Form.Item label={t('field.verify')} style={{ marginBottom: 8 }}><Switch checked={outboundTls.verify ?? true} disabled={readOnly} onChange={(verify) => updateSpecBlock('outboundTls', { verify })} /></Form.Item>
          <Form.Item label={t('field.caRefs')} style={{ marginBottom: 8 }}><ObjectRefsEditor value={outboundValidation.caCertificateRefs || []} disabled={readOnly} onChange={(caCertificateRefs) => updateSpecBlock('outboundTls', { validation: { ...outboundValidation, caCertificateRefs } })} /></Form.Item>
          <Form.Item label={t('field.wellKnownCa')} style={{ marginBottom: 8 }}><Select allowClear value={outboundValidation.wellKnownCACertificates} options={[{ value: 'System' }]} disabled={readOnly} onChange={(wellKnownCACertificates) => updateSpecBlock('outboundTls', { validation: { ...outboundValidation, wellKnownCACertificates } })} /></Form.Item>
          <Form.Item label={t('field.validationHostname')} style={{ marginBottom: 8 }}><Input value={outboundValidation.hostname || ''} disabled={readOnly} onChange={(event) => updateSpecBlock('outboundTls', { validation: { ...outboundValidation, hostname: event.target.value || undefined } })} /></Form.Item>
          <Form.Item label={t('field.subjectAltNames')} style={{ marginBottom: 8 }}>
            {(outboundValidation.subjectAltNames || []).map((san: SubjectAltName, index: number) => <Space key={index} wrap style={{ marginBottom: 8 }}><Select value={san.type} options={[{ value: 'Hostname' }, { value: 'URI' }]} disabled={readOnly} onChange={(type) => updateSpecBlock('outboundTls', { validation: { ...outboundValidation, subjectAltNames: (outboundValidation.subjectAltNames || []).map((item, itemIndex) => itemIndex === index ? { ...item, type } : item) } })} /><Input value={san.type === 'URI' ? san.uri || '' : san.hostname || ''} disabled={readOnly} onChange={(event) => updateSpecBlock('outboundTls', { validation: { ...outboundValidation, subjectAltNames: (outboundValidation.subjectAltNames || []).map((item, itemIndex) => itemIndex === index ? { ...item, [item.type === 'URI' ? 'uri' : 'hostname']: event.target.value } : item) } })} />{!readOnly && <Button danger type="text" icon={<MinusCircleOutlined />} onClick={() => updateSpecBlock('outboundTls', { validation: { ...outboundValidation, subjectAltNames: (outboundValidation.subjectAltNames || []).filter((_, itemIndex) => itemIndex !== index) } })} />}</Space>)}
            {!readOnly && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => updateSpecBlock('outboundTls', { validation: { ...outboundValidation, subjectAltNames: [...(outboundValidation.subjectAltNames || []), { type: 'Hostname', hostname: '' }] } })}>{t('btn.addSubjectAltName')}</Button>}
          </Form.Item>
          <Form.Item label={t('field.clientCertificateRef')} style={{ marginBottom: 0 }}><ObjectRefsEditor value={outboundTls.clientCertificateRef ? [outboundTls.clientCertificateRef] : []} disabled={readOnly} maxItems={1} onChange={(items) => updateSpecBlock('outboundTls', { clientCertificateRef: items[0] })} /></Form.Item>
        </Card>

        <Card title={t('section.dnsResolver')} size="small">
          <Form.Item label={t('field.dnsResolverSource')} style={{ marginBottom: 8 }}><Select allowClear value={dnsResolverMode} disabled={readOnly} options={[{ value: 'servers', label: t('field.dnsServers') }, { value: 'linkSysRef', label: 'LinkSys' }]} onChange={(mode) => updateSpecBlock('dnsResolver', mode === 'servers' ? { linkSysRef: undefined, servers: dnsResolver.servers || [] } : mode === 'linkSysRef' ? { servers: undefined, linkSysRef: dnsResolver.linkSysRef || { namespace: '', name: '' } } : { servers: undefined, linkSysRef: undefined })} /></Form.Item>
          {dnsResolverMode === 'servers' && <Form.Item label={t('field.dnsServers')} style={{ marginBottom: 8 }}><Select mode="tags" value={dnsResolver.servers || []} disabled={readOnly} tokenSeparators={[',']} onChange={(servers) => updateSpecBlock('dnsResolver', { servers })} /></Form.Item>}
          <Form.Item label={t('field.cacheTtl')} style={{ marginBottom: 8 }}><Input value={dnsResolver.cacheTtl || ''} disabled={readOnly} onChange={(event) => updateSpecBlock('dnsResolver', { cacheTtl: event.target.value || undefined })} /></Form.Item>
          {dnsResolverMode === 'linkSysRef' && <Space wrap><Form.Item label={t('field.linkSysNamespace')} style={{ marginBottom: 0 }}><Input value={dnsResolver.linkSysRef?.namespace || ''} disabled={readOnly} onChange={(event) => updateSpecBlock('dnsResolver', { linkSysRef: { ...dnsResolver.linkSysRef, namespace: event.target.value, name: dnsResolver.linkSysRef?.name || '' } })} /></Form.Item><Form.Item label={t('field.linkSysName')} style={{ marginBottom: 0 }}><Input value={dnsResolver.linkSysRef?.name || ''} disabled={readOnly} onChange={(event) => updateSpecBlock('dnsResolver', { linkSysRef: { ...dnsResolver.linkSysRef, namespace: dnsResolver.linkSysRef?.namespace || '', name: event.target.value } })} /></Form.Item></Space>}
        </Card>
      </Space>
    </Form>
  )
}

export default EdgionGatewayConfigForm
