/**
 * 单个 Listener 编辑器 — 按 protocol 条件渲染 TLS 配置
 */

import React from 'react'
import { AutoComplete, Card, Form, Input, InputNumber, Select, Button, Space } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import type { CertificateRef, FrontendTLSValidation, GatewayListener } from '@/types/gateway-api/gateway'
import { useT } from '@/i18n'

interface ListenerEditorProps {
  listener: GatewayListener
  index: number
  canRemove: boolean
  onChange: (listener: GatewayListener) => void
  onRemove: () => void
  disabled?: boolean
}

export function replaceCertificateRef(
  listener: GatewayListener,
  index: number,
  patch: Partial<CertificateRef>,
): GatewayListener {
  const certificateRefs = [...(listener.tls?.certificateRefs || [])]
  certificateRefs[index] = { ...(certificateRefs[index] || { name: '' }), ...patch }
  return { ...listener, tls: { ...listener.tls, certificateRefs } }
}

interface ReferenceListProps {
  value: CertificateRef[]
  onChange: (value: CertificateRef[]) => void
  disabled: boolean
  title: string
}

const ReferenceList: React.FC<ReferenceListProps> = ({ value, onChange, disabled, title }) => {
  const t = useT()
  return <div>
    {value.map((ref, refIndex) => <Card key={refIndex} type="inner" size="small" title={`${title} ${refIndex + 1}`} style={{ marginBottom: 8 }} extra={!disabled && <Button type="text" danger aria-label={t('btn.deleteReference')} icon={<MinusCircleOutlined />} onClick={() => onChange(value.filter((_, index) => index !== refIndex))} />}>
      <Space wrap>
        <Form.Item label={t('field.name')} required style={{ marginBottom: 0 }}><Input value={ref.name} disabled={disabled} onChange={(event) => onChange(value.map((item, index) => index === refIndex ? { ...item, name: event.target.value } : item))} /></Form.Item>
        <Form.Item label={t('field.namespaceOpt')} style={{ marginBottom: 0 }}><Input value={ref.namespace || ''} disabled={disabled} onChange={(event) => onChange(value.map((item, index) => index === refIndex ? { ...item, namespace: event.target.value || undefined } : item))} /></Form.Item>
        <Form.Item label={t('field.group')} style={{ marginBottom: 0 }}><Input value={ref.group || ''} disabled={disabled} onChange={(event) => onChange(value.map((item, index) => index === refIndex ? { ...item, group: event.target.value } : item))} /></Form.Item>
        <Form.Item label={t('field.kind')} style={{ marginBottom: 0 }}><Input value={ref.kind || ''} disabled={disabled} onChange={(event) => onChange(value.map((item, index) => index === refIndex ? { ...item, kind: event.target.value || undefined } : item))} /></Form.Item>
      </Space>
    </Card>)}
    {!disabled && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => onChange([...value, { name: '', group: '', kind: 'Secret' }])}>{t('btn.addReference')}</Button>}
  </div>
}

function updateStringMap(map: Record<string, unknown>, oldKey: string, key: string, value: unknown): Record<string, unknown> {
  const next = { ...map }
  if (oldKey !== key) delete next[oldKey]
  next[key] = value
  return next
}

const ListenerEditor: React.FC<ListenerEditorProps> = ({
  listener, index, canRemove, onChange, onRemove, disabled = false,
}) => {
  const t = useT()
  const update = (partial: Partial<GatewayListener>) => onChange({ ...listener, ...partial })
  const needsTLS = listener.protocol === 'HTTPS' || listener.protocol === 'TLS'
  const certificateRefs = listener.tls?.certificateRefs || []

  return (
    <Card
      type="inner"
      size="small"
      title={t('gw.listenerTitle', { n: index + 1, name: listener.name || t('gw.unnamed') })}
      extra={
        !disabled && canRemove && (
          <Button danger size="small" icon={<MinusCircleOutlined />} onClick={onRemove}>{t('btn.delete')}</Button>
        )
      }
      style={{ marginBottom: 12 }}
    >
      <Space direction="vertical" style={{ width: '100%' }}>
        <Space wrap>
          <Form.Item label={t('field.name')} required style={{ marginBottom: 0 }}>
            <Input
              value={listener.name}
              onChange={(e) => update({ name: e.target.value })}
              placeholder="http"
              disabled={disabled}
              style={{ width: 160 }}
            />
          </Form.Item>
          <Form.Item label={t('field.protocol')} required style={{ marginBottom: 0 }}>
            <AutoComplete
              value={listener.protocol}
              onChange={(v) => update({ protocol: v })}
              disabled={disabled}
              style={{ width: 180 }}
              options={['HTTP', 'HTTPS', 'TCP', 'TLS', 'UDP'].map((value) => ({ value }))}
            />
          </Form.Item>
          <Form.Item label={t('field.port')} required style={{ marginBottom: 0 }}>
            <InputNumber
              value={listener.port}
              onChange={(v) => update({ port: v || 80 })}
              min={1} max={65535}
              disabled={disabled}
              style={{ width: 100 }}
            />
          </Form.Item>
          <Form.Item label={t('field.hostnameOpt')} style={{ marginBottom: 0 }}>
            <Input
              value={listener.hostname || ''}
              onChange={(e) => update({ hostname: e.target.value || undefined })}
              placeholder="*.example.com"
              disabled={disabled}
              style={{ width: 200 }}
            />
          </Form.Item>
        </Space>

        {needsTLS && (
          <Card title={t('gw.tlsConfig')} size="small" type="inner">
            <Form.Item label={t('field.tlsMode')} style={{ marginBottom: 8 }}>
              <Select
                value={listener.tls?.mode || 'Terminate'}
                onChange={(v) => update({ tls: { ...listener.tls, mode: v } })}
                disabled={disabled}
                style={{ width: 160 }}
              >
                <Select.Option value="Terminate">{t('gw.terminate')}</Select.Option>
                <Select.Option value="Passthrough">{t('gw.passthrough')}</Select.Option>
              </Select>
            </Form.Item>

            {listener.tls?.mode !== 'Passthrough' && (
              <>
                <ReferenceList value={certificateRefs} disabled={disabled} title={t('gw.certificate')} onChange={(certificateRefs) => update({ tls: { ...listener.tls, certificateRefs } })} />
              </>
            )}

            <Card title={t('gw.frontendValidation')} size="small" type="inner" style={{ marginTop: 8 }}>
              <Form.Item label={t('field.validationMode')} style={{ marginBottom: 8 }}>
                <Select
                  allowClear
                  value={listener.tls?.frontendValidation?.mode}
                  options={[{ value: 'AllowValidOnly' }, { value: 'AllowInsecureFallback' }]}
                  onChange={(mode) => update({ tls: { ...listener.tls, frontendValidation: { ...listener.tls?.frontendValidation, mode } as FrontendTLSValidation } })}
                  disabled={disabled}
                  style={{ width: 220 }}
                />
              </Form.Item>
              <ReferenceList value={listener.tls?.frontendValidation?.caCertificateRefs || []} disabled={disabled} title={t('gw.caReference')} onChange={(caCertificateRefs) => update({ tls: { ...listener.tls, frontendValidation: { ...listener.tls?.frontendValidation, caCertificateRefs } } })} />
            </Card>

            <Card title={t('gw.tlsOptions')} size="small" type="inner" style={{ marginTop: 8 }}>
              {Object.entries(listener.tls?.options || {}).map(([key, value], optionIndex) => <Space key={`${key}-${optionIndex}`} wrap style={{ marginBottom: 8 }}>
                <Input aria-label={t('field.optionKey')} value={key} disabled={disabled} onChange={(event) => update({ tls: { ...listener.tls, options: updateStringMap(listener.tls?.options || {}, key, event.target.value, value) } })} />
                <Input aria-label={t('field.optionValue')} value={String(value ?? '')} disabled={disabled} onChange={(event) => update({ tls: { ...listener.tls, options: updateStringMap(listener.tls?.options || {}, key, key, event.target.value) } })} />
                {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} aria-label={t('btn.deleteOption')} onClick={() => { const options = { ...(listener.tls?.options || {}) }; delete options[key]; update({ tls: { ...listener.tls, options } }) }} />}
              </Space>)}
              {!disabled && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => update({ tls: { ...listener.tls, options: { ...(listener.tls?.options || {}), '': '' } } })}>{t('btn.addOption')}</Button>}
            </Card>
          </Card>
        )}

        <Card title={t('gw.allowedRoutes')} size="small" type="inner">
          <Form.Item label={t('field.namespacePolicy')} style={{ marginBottom: 8 }}>
            <Select
              allowClear
              value={listener.allowedRoutes?.namespaces?.from}
              options={[{ value: 'Same' }, { value: 'All' }, { value: 'Selector' }]}
              onChange={(from) => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, from } } })}
              disabled={disabled}
              style={{ width: 180 }}
            />
          </Form.Item>
          {listener.allowedRoutes?.namespaces?.from === 'Selector' && <div>
            {Object.entries(listener.allowedRoutes.namespaces.selector?.matchLabels || {}).map(([key, value], labelIndex) => <Space key={`${key}-${labelIndex}`} style={{ marginBottom: 8 }}>
              <Input aria-label={t('field.labelKey')} value={key} disabled={disabled} onChange={(event) => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchLabels: updateStringMap(listener.allowedRoutes?.namespaces?.selector?.matchLabels || {}, key, event.target.value, value) as Record<string, string> } } } })} />
              <Input aria-label={t('field.labelValue')} value={value} disabled={disabled} onChange={(event) => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchLabels: updateStringMap(listener.allowedRoutes?.namespaces?.selector?.matchLabels || {}, key, key, event.target.value) as Record<string, string> } } } })} />
              {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} aria-label={t('btn.deleteLabel')} onClick={() => { const matchLabels = { ...(listener.allowedRoutes?.namespaces?.selector?.matchLabels || {}) }; delete matchLabels[key]; update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchLabels } } } }) }} />}
            </Space>)}
            {!disabled && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchLabels: { ...(listener.allowedRoutes?.namespaces?.selector?.matchLabels || {}), '': '' } } } } })}>{t('btn.addLabel')}</Button>}
            <Card title={t('gw.matchExpressions')} size="small" type="inner" style={{ marginTop: 8 }}>
              {(listener.allowedRoutes.namespaces.selector?.matchExpressions || []).map((expression, expressionIndex) => <Space key={expressionIndex} wrap style={{ marginBottom: 8 }}>
                <Input aria-label={t('field.labelKey')} value={expression.key} disabled={disabled} onChange={(event) => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchExpressions: (listener.allowedRoutes?.namespaces?.selector?.matchExpressions || []).map((item, index) => index === expressionIndex ? { ...item, key: event.target.value } : item) } } } })} />
                <Select value={expression.operator} disabled={disabled} options={['In', 'NotIn', 'Exists', 'DoesNotExist'].map((value) => ({ value }))} onChange={(operator) => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchExpressions: (listener.allowedRoutes?.namespaces?.selector?.matchExpressions || []).map((item, index) => index === expressionIndex ? { ...item, operator } : item) } } } })} style={{ width: 150 }} />
                <Select mode="tags" value={expression.values || []} disabled={disabled} tokenSeparators={[',']} onChange={(values) => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchExpressions: (listener.allowedRoutes?.namespaces?.selector?.matchExpressions || []).map((item, index) => index === expressionIndex ? { ...item, values } : item) } } } })} style={{ minWidth: 220 }} />
                {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} aria-label={t('btn.deleteExpression')} onClick={() => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchExpressions: (listener.allowedRoutes?.namespaces?.selector?.matchExpressions || []).filter((_, index) => index !== expressionIndex) } } } })} />}
              </Space>)}
              {!disabled && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => update({ allowedRoutes: { ...listener.allowedRoutes, namespaces: { ...listener.allowedRoutes?.namespaces, selector: { ...listener.allowedRoutes?.namespaces?.selector, matchExpressions: [...(listener.allowedRoutes?.namespaces?.selector?.matchExpressions || []), { key: '', operator: 'In', values: [] }] } } } })}>{t('btn.addExpression')}</Button>}
            </Card>
          </div>}
          <Card title={t('gw.allowedKinds')} size="small" type="inner" style={{ marginTop: 8 }}>
            {(listener.allowedRoutes?.kinds || []).map((kind, kindIndex) => <Space key={kindIndex} wrap style={{ marginBottom: 8 }}>
              <Input aria-label={t('field.group')} value={kind.group || ''} disabled={disabled} onChange={(event) => update({ allowedRoutes: { ...listener.allowedRoutes, kinds: (listener.allowedRoutes?.kinds || []).map((item, index) => index === kindIndex ? { ...item, group: event.target.value || undefined } : item) } })} />
              <Input aria-label={t('field.kind')} value={kind.kind} disabled={disabled} onChange={(event) => update({ allowedRoutes: { ...listener.allowedRoutes, kinds: (listener.allowedRoutes?.kinds || []).map((item, index) => index === kindIndex ? { ...item, kind: event.target.value } : item) } })} />
              {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} aria-label={t('btn.deleteKind')} onClick={() => update({ allowedRoutes: { ...listener.allowedRoutes, kinds: (listener.allowedRoutes?.kinds || []).filter((_, index) => index !== kindIndex) } })} />}
            </Space>)}
            {!disabled && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => update({ allowedRoutes: { ...listener.allowedRoutes, kinds: [...(listener.allowedRoutes?.kinds || []), { group: 'gateway.networking.k8s.io', kind: 'HTTPRoute' }] } })}>{t('btn.addKind')}</Button>}
          </Card>
        </Card>
      </Space>
    </Card>
  )
}

export default ListenerEditor
