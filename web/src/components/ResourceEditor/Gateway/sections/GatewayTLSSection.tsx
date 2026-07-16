import { Button, Card, Form, Input, InputNumber, Select, Space, Switch } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import type { CertificateRef, FrontendTLSValidation, GatewaySpecTLS } from '@/types/gateway-api/gateway'
import { useT } from '@/i18n'

interface Props {
  value?: GatewaySpecTLS
  onChange: (value: GatewaySpecTLS | undefined) => void
  disabled?: boolean
}

function ReferenceEditor({ value, onChange, disabled, multiple = true }: {
  value: CertificateRef[]
  onChange: (value: CertificateRef[]) => void
  disabled: boolean
  multiple?: boolean
}) {
  const t = useT()
  return <Space direction="vertical" style={{ width: '100%' }}>
    {value.map((ref, index) => <Space key={index} wrap>
      <Input aria-label={t('field.name')} value={ref.name} disabled={disabled} onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item))} />
      <Input aria-label={t('field.namespaceOpt')} value={ref.namespace || ''} disabled={disabled} onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, namespace: event.target.value || undefined } : item))} />
      <Input aria-label={t('field.group')} value={ref.group || ''} disabled={disabled} onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, group: event.target.value } : item))} />
      <Input aria-label={t('field.kind')} value={ref.kind || ''} disabled={disabled} onChange={(event) => onChange(value.map((item, itemIndex) => itemIndex === index ? { ...item, kind: event.target.value || undefined } : item))} />
      {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} aria-label={t('btn.deleteReference')} onClick={() => onChange(value.filter((_, itemIndex) => itemIndex !== index))} />}
    </Space>)}
    {!disabled && (multiple || value.length === 0) && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => onChange([...value, { name: '', group: '', kind: 'Secret' }])}>{t('btn.addReference')}</Button>}
  </Space>
}

function ValidationEditor({ value, onChange, disabled }: { value?: FrontendTLSValidation; onChange: (value: FrontendTLSValidation) => void; disabled: boolean }) {
  const t = useT()
  return <>
    <Form.Item label={t('field.validationMode')} style={{ marginBottom: 8 }}><Select allowClear value={value?.mode} disabled={disabled} options={[{ value: 'AllowValidOnly' }, { value: 'AllowInsecureFallback' }]} onChange={(mode) => onChange({ ...value, mode })} /></Form.Item>
    <Form.Item label={t('field.caRefs')} style={{ marginBottom: 0 }}><ReferenceEditor value={value?.caCertificateRefs || []} disabled={disabled} onChange={(caCertificateRefs) => onChange({ ...value, caCertificateRefs })} /></Form.Item>
  </>
}

export default function GatewayTLSSection({ value, onChange, disabled = false }: Props) {
  const t = useT()
  const enabled = !!value
  const perPort = value?.frontend?.perPort || []
  return <Card title={t('section.gatewayTls')} size="small">
    <Form.Item label={t('field.gatewayTlsEnabled')} style={{ marginBottom: 8 }}><Switch checked={enabled} disabled={disabled} onChange={(checked) => onChange(checked ? {} : undefined)} /></Form.Item>
    {enabled && <Space direction="vertical" style={{ width: '100%' }}>
      <Card title={t('section.gatewayBackendTls')} size="small" type="inner">
        <Form.Item label={t('field.clientCertificateRef')} style={{ marginBottom: 0 }}><ReferenceEditor multiple={false} value={value?.backend?.clientCertificateRef ? [value.backend.clientCertificateRef] : []} disabled={disabled} onChange={(refs) => onChange({ ...value, backend: { ...value?.backend, clientCertificateRef: refs[0] } })} /></Form.Item>
      </Card>
      <Card title={t('section.gatewayFrontendDefault')} size="small" type="inner">
        <ValidationEditor value={value?.frontend?.default?.validation} disabled={disabled} onChange={(validation) => onChange({ ...value, frontend: { ...value?.frontend, default: { ...value?.frontend?.default, validation } } })} />
      </Card>
      <Card title={t('section.gatewayFrontendPerPort')} size="small" type="inner">
        {perPort.map((item, index) => <Card key={index} size="small" type="inner" title={t('gw.portOverride', { n: index + 1 })} extra={!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} aria-label={t('btn.deletePortOverride')} onClick={() => onChange({ ...value, frontend: { ...value?.frontend, perPort: perPort.filter((_, itemIndex) => itemIndex !== index) } })} />} style={{ marginBottom: 8 }}>
          <Form.Item label={t('field.port')} required style={{ marginBottom: 8 }}><InputNumber min={1} max={65535} value={item.port} disabled={disabled} onChange={(port) => onChange({ ...value, frontend: { ...value?.frontend, perPort: perPort.map((entry, itemIndex) => itemIndex === index ? { ...entry, port: port || 0 } : entry) } })} /></Form.Item>
          <ValidationEditor value={item.tls?.validation} disabled={disabled} onChange={(validation) => onChange({ ...value, frontend: { ...value?.frontend, perPort: perPort.map((entry, itemIndex) => itemIndex === index ? { ...entry, tls: { ...entry.tls, validation } } : entry) } })} />
        </Card>)}
        {!disabled && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => onChange({ ...value, frontend: { ...value?.frontend, perPort: [...perPort, { port: 443, tls: { validation: { mode: 'AllowValidOnly', caCertificateRefs: [] } } }] } })}>{t('btn.addPortOverride')}</Button>}
      </Card>
    </Space>}
  </Card>
}
