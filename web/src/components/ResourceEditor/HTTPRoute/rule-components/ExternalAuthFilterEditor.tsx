import React from 'react'
import { Button, Card, Checkbox, Form, Input, InputNumber, Select, Space } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import type { HTTPExternalAuthFilter } from '@/types/gateway-api/httproute'
import { useT } from '@/i18n'

interface Props {
  value: HTTPExternalAuthFilter
  onChange: (value: HTTPExternalAuthFilter) => void
  disabled: boolean
}

const label = (t: ReturnType<typeof useT>, field: string) => t('externalAuth.field', { field })

function StringList({ value = [], onChange, disabled }: { value?: string[]; onChange: (value: string[]) => void; disabled: boolean }) {
  return <Select mode="tags" tokenSeparators={[',']} value={value} onChange={onChange} disabled={disabled} style={{ minWidth: 240 }} />
}

function RefRows({ value = [], onChange, disabled }: { value?: Record<string, unknown>[]; onChange: (value: Record<string, unknown>[]) => void; disabled: boolean }) {
  const t = useT()
  const update = (index: number, patch: Record<string, unknown>) => {
    const next = [...value]
    next[index] = { ...next[index], ...patch }
    onChange(next)
  }
  return <Space direction="vertical" style={{ width: '100%' }}>
    {value.map((ref, index) => <Space key={index} wrap>
      {['group', 'kind', 'namespace', 'name'].map((field) => <Input key={field} aria-label={`${field}-${index}`} value={String(ref[field] || '')} placeholder={field} onChange={(e) => update(index, { [field]: e.target.value })} disabled={disabled} />)}
      {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} onClick={() => onChange(value.filter((_, i) => i !== index))} />}
    </Space>)}
    {!disabled && <Button type="dashed" icon={<PlusOutlined />} onClick={() => onChange([...value, { name: '' }])}>{t('btn.addDataEntry')}</Button>}
  </Space>
}

type SanType = 'Hostname' | 'URI'

export function switchSubjectAltNameType(current: Record<string, unknown>, type: SanType): Record<string, unknown> {
  const next: Record<string, unknown> = { ...current, type }
  delete next.hostname
  delete next.uri
  next[type === 'Hostname' ? 'hostname' : 'uri'] = ''
  return next
}

function SubjectAltNameRows({ value = [], onChange, disabled }: { value?: Record<string, unknown>[]; onChange: (value: Record<string, unknown>[]) => void; disabled: boolean }) {
  const t = useT()
  const update = (index: number, san: Record<string, unknown>) => {
    const next = [...value]
    next[index] = san
    onChange(next)
  }
  return <Space direction="vertical" style={{ width: '100%' }}>
    {value.map((san, index) => {
      const type: SanType = san.type === 'URI' ? 'URI' : 'Hostname'
      const field = type === 'URI' ? 'uri' : 'hostname'
      return <Space key={index} wrap>
        <Select aria-label={`san-${index}-type`} value={type} options={['Hostname', 'URI'].map((item) => ({ value: item }))} onChange={(nextType: SanType) => update(index, switchSubjectAltNameType(san, nextType))} disabled={disabled} style={{ width: 130 }} />
        <Input aria-label={`san-${index}-${field}`} value={String(san[field] || '')} onChange={(event) => update(index, { ...san, [field]: event.target.value })} disabled={disabled} />
        {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} onClick={() => onChange(value.filter((_, i) => i !== index))} />}
      </Space>
    })}
    {!disabled && <Button type="dashed" icon={<PlusOutlined />} onClick={() => onChange([...value, { type: 'Hostname', hostname: '' }])}>{t('externalAuth.addSan')}</Button>}
  </Space>
}

export function parseJsonValueInput(input: string): unknown {
  return JSON.parse(input)
}

function JsonValueInput({ value, onChange, disabled, ariaLabel, validate }: { value: unknown; onChange: (value: unknown) => void; disabled: boolean; ariaLabel: string; validate?: (value: unknown) => boolean }) {
  const [draft, setDraft] = React.useState(() => JSON.stringify(value))
  const [invalid, setInvalid] = React.useState(false)
  React.useEffect(() => {
    setDraft(JSON.stringify(value))
    setInvalid(false)
  }, [value])
  return <Input.TextArea
    aria-label={ariaLabel}
    value={draft}
    status={invalid ? 'error' : undefined}
    onChange={(event) => {
      const next = event.target.value
      setDraft(next)
      try {
        const parsed = parseJsonValueInput(next)
        if (validate && !validate(parsed)) {
          setInvalid(true)
          return
        }
        onChange(parsed)
        setInvalid(false)
      } catch {
        setInvalid(true)
      }
    }}
    disabled={disabled}
    autoSize={{ minRows: 1, maxRows: 4 }}
  />
}

function ListFieldsEditor({ value = {}, onChange, disabled, allowForwardAll }: { value?: Record<string, any>; onChange: (value: Record<string, any>) => void; disabled: boolean; allowForwardAll: boolean }) {
  const t = useT()
  const origins = value.origin || []
  const custom = value.custom || []
  return <Space direction="vertical" style={{ width: '100%' }}>
    {allowForwardAll && <Checkbox checked={value.forwardAll ?? false} onChange={(e) => onChange({ ...value, forwardAll: e.target.checked })} disabled={disabled}>{label(t, 'forwardAll')}</Checkbox>}
    {origins.map((item: any, index: number) => <Space key={`origin-${index}`} wrap>
      <Input value={item.name || ''} placeholder="origin.name" onChange={(e) => { const next = [...origins]; next[index] = { ...item, name: e.target.value }; onChange({ ...value, origin: next }) }} disabled={disabled} />
      <Select value={item.presence || 'skipIfMissing'} options={['skipIfMissing','required','warnIfMissing'].map((v) => ({ value: v }))} onChange={(presence) => { const next = [...origins]; next[index] = { ...item, presence }; onChange({ ...value, origin: next }) }} disabled={disabled} style={{ width: 150 }} />
      {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} onClick={() => onChange({ ...value, origin: origins.filter((_: unknown, i: number) => i !== index) })} />}
    </Space>)}
    {!disabled && <Button size="small" type="dashed" onClick={() => onChange({ ...value, origin: [...origins, { name: '', presence: 'skipIfMissing' }] })}>{t('externalAuth.addOrigin')}</Button>}
    {custom.map((item: any, index: number) => {
      const source = Object.prototype.hasOwnProperty.call(item, 'template') ? 'template' : 'value'
      return <Space key={`custom-${index}`} wrap>
        <Input value={item.name || ''} placeholder="custom.name" onChange={(e) => { const next = [...custom]; next[index] = { ...item, name: e.target.value }; onChange({ ...value, custom: next }) }} disabled={disabled} />
        <Select value={source} options={['value','template'].map((v) => ({ value: v }))} onChange={(nextSource) => { const next = [...custom]; next[index] = { name: item.name || '', [nextSource]: '' }; onChange({ ...value, custom: next }) }} disabled={disabled} style={{ width: 110 }} />
        <Input value={item[source] || ''} placeholder={source} onChange={(e) => { const next = [...custom]; next[index] = { ...item, [source]: e.target.value }; onChange({ ...value, custom: next }) }} disabled={disabled} />
        {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} onClick={() => onChange({ ...value, custom: custom.filter((_: unknown, i: number) => i !== index) })} />}
      </Space>
    })}
    {!disabled && <Button size="small" type="dashed" onClick={() => onChange({ ...value, custom: [...custom, { name: '', value: '' }] })}>{t('externalAuth.addCustom')}</Button>}
  </Space>
}

const ExternalAuthFilterEditor: React.FC<Props> = ({ value, onChange, disabled }) => {
  const t = useT()
  const target = (value.target || {}) as Record<string, any>
  const targetMode = target.url ? 'url' : 'service'
  const tls = (value as any).tls || {}
  const retry = (value as any).retry || {}
  const rateLimit = (value as any).rateLimit || {}
  const health = (value as any).healthCheck || {}
  const active = health.active || {}
  const passive = health.passive || {}
  const success = (value as any).success || {}
  const request = value.request || {}
  const decision = value.decision || {}
  const patch = (next: Record<string, unknown>) => onChange({ ...value, ...next })
  const numberField = (object: Record<string, any>, field: string, update: (next: Record<string, any>) => void, min = 0, max?: number) =>
    <Form.Item label={label(t, field)} style={{ marginBottom: 0 }}><InputNumber value={object[field]} min={min} max={max} onChange={(v) => update({ ...object, [field]: v ?? undefined })} disabled={disabled} /></Form.Item>
  const textField = (object: Record<string, any>, field: string, update: (next: Record<string, any>) => void) =>
    <Form.Item label={label(t, field)} style={{ marginBottom: 0 }}><Input value={object[field] || ''} onChange={(e) => update({ ...object, [field]: e.target.value })} disabled={disabled} /></Form.Item>

  return <Space direction="vertical" style={{ width: '100%' }}>
    <Card size="small" title={t('externalAuth.target')}>
      <Space direction="vertical" style={{ width: '100%' }}>
        <Select value={targetMode} options={['service','url'].map((v) => ({ value: v }))} onChange={(mode) => patch({ target: mode === 'url' ? { url: '' } : { name: '', port: 80 } })} disabled={disabled} style={{ width: 140 }} />
        {targetMode === 'url' ? <Space wrap>{textField(target, 'url', (next) => patch({ target: next }))}<Checkbox checked={target.blockPrivate} onChange={(e) => patch({ target: { ...target, blockPrivate: e.target.checked } })} disabled={disabled}>{label(t, 'blockPrivate')}</Checkbox></Space> : <Space wrap>
          {['name','namespace','group','kind'].map((field) => <React.Fragment key={field}>{textField(target, field, (next) => patch({ target: next }))}</React.Fragment>)}
          {numberField(target, 'port', (next) => patch({ target: next }), 1, 65535)}
        </Space>}
      </Space>
    </Card>

    <Card size="small" title={t('externalAuth.connection')}><Space wrap>
      {numberField(value as any, 'timeoutMs', patch, 1, 60000)}
      {textField(value as any, 'timeoutMsTemplate', patch)}
      {numberField(value as any, 'maxResponseBytes', patch, 1)}
      {numberField(value as any, 'statusOnError', patch, 200, 599)}
      <Checkbox checked={value.allowDegradation ?? false} onChange={(e) => patch({ allowDegradation: e.target.checked })} disabled={disabled}>{label(t, 'allowDegradation')}</Checkbox>
      {textField(value as any, 'allowDegradationTemplate', patch)}
    </Space></Card>

    <Card size="small" title={t('externalAuth.tls')}><Space direction="vertical" style={{ width: '100%' }}>
      <Space wrap><Checkbox checked={tls.enabled ?? false} onChange={(e) => patch({ tls: { ...tls, enabled: e.target.checked } })} disabled={disabled}>{label(t, 'enabled')}</Checkbox><Checkbox checked={tls.verify ?? true} onChange={(e) => patch({ tls: { ...tls, verify: e.target.checked } })} disabled={disabled}>{label(t, 'verify')}</Checkbox></Space>
      <Form.Item label={label(t, 'validation.caCertificateRefs')} style={{ marginBottom: 0 }}><RefRows value={tls.validation?.caCertificateRefs || []} onChange={(caCertificateRefs) => patch({ tls: { ...tls, validation: { ...tls.validation, caCertificateRefs } } })} disabled={disabled} /></Form.Item>
      <Space wrap>
        {textField(tls.validation || {}, 'hostname', (validation) => patch({ tls: { ...tls, validation } }))}
        <Form.Item label={label(t, 'wellKnownCACertificates')} style={{ marginBottom: 0 }}><Select allowClear value={tls.validation?.wellKnownCACertificates} options={[{ value: 'System' }]} onChange={(wellKnownCACertificates) => patch({ tls: { ...tls, validation: { ...tls.validation, wellKnownCACertificates } } })} disabled={disabled} /></Form.Item>
        <Form.Item label={label(t, 'subjectAltNames')} style={{ marginBottom: 0 }}><SubjectAltNameRows value={tls.validation?.subjectAltNames || []} onChange={(subjectAltNames) => patch({ tls: { ...tls, validation: { ...tls.validation, subjectAltNames } } })} disabled={disabled} /></Form.Item>
      </Space>
      <Form.Item label={label(t, 'clientCertificateRef')} style={{ marginBottom: 0 }}><RefRows value={tls.clientCertificateRef ? [tls.clientCertificateRef] : []} onChange={(refs) => patch({ tls: { ...tls, clientCertificateRef: refs[0] } })} disabled={disabled} /></Form.Item>
    </Space></Card>

    <Card size="small" title={t('externalAuth.retry')}><Space wrap>
      {numberField(retry, 'maxRetries', (next) => patch({ retry: next }), 0, 10)}
      {numberField(retry, 'retryDelayMs', (next) => patch({ retry: next }), 1)}
      {numberField(retry, 'maxDelayMs', (next) => patch({ retry: next }), 1, 10000)}
      <Form.Item label={label(t, 'backoffPolicy')} style={{ marginBottom: 0 }}><Select value={retry.backoffPolicy} options={['exponential','fixed'].map((v) => ({ value: v }))} onChange={(backoffPolicy) => patch({ retry: { ...retry, backoffPolicy } })} disabled={disabled} /></Form.Item>
      <Form.Item label={label(t, 'retryOnStatus')} style={{ marginBottom: 0 }}><StringList value={(retry.retryOnStatus || []).map(String)} onChange={(codes) => patch({ retry: { ...retry, retryOnStatus: codes.map(Number).filter((v) => v >= 100 && v <= 599) } })} disabled={disabled} /></Form.Item>
      {['retryOnTimeout','retryOnConnectError','jitter','honorRetryAfter','retryOnBodyFailure'].map((field) => <Checkbox key={field} checked={retry[field] ?? false} onChange={(e) => patch({ retry: { ...retry, [field]: e.target.checked } })} disabled={disabled}>{label(t, field)}</Checkbox>)}
    </Space></Card>

    <Card size="small" title={t('externalAuth.rateLimit')}><Space wrap>{numberField(rateLimit, 'rate', (next) => patch({ rateLimit: next }), 1)}{numberField(rateLimit, 'windowSec', (next) => patch({ rateLimit: next }), 1)}</Space></Card>

    <Card size="small" title={t('externalAuth.healthCheck')}><Space direction="vertical" style={{ width: '100%' }}>
      <Card size="small" title="active"><Space wrap>{textField(active, 'path', (next) => patch({ healthCheck: { ...health, active: next } }))}{numberField(active, 'intervalSec', (next) => patch({ healthCheck: { ...health, active: next } }), 1)}{numberField(active, 'timeoutMs', (next) => patch({ healthCheck: { ...health, active: next } }), 1)}{numberField(active, 'healthyThreshold', (next) => patch({ healthCheck: { ...health, active: next } }), 1)}{numberField(active, 'unhealthyThreshold', (next) => patch({ healthCheck: { ...health, active: next } }), 1)}</Space></Card>
      <Card size="small" title="passive"><Space wrap>{numberField(passive, 'unhealthyThreshold', (next) => patch({ healthCheck: { ...health, passive: next } }), 1)}<Form.Item label={label(t, 'failureStatusCodes')} style={{ marginBottom: 0 }}><StringList value={(passive.failureStatusCodes || []).map(String)} onChange={(codes) => patch({ healthCheck: { ...health, passive: { ...passive, failureStatusCodes: codes.map(Number) } } })} disabled={disabled} /></Form.Item><Checkbox checked={passive.countTimeout ?? false} onChange={(e) => patch({ healthCheck: { ...health, passive: { ...passive, countTimeout: e.target.checked } } })} disabled={disabled}>{label(t, 'countTimeout')}</Checkbox>{numberField(passive.backoff || {}, 'initialSec', (backoff) => patch({ healthCheck: { ...health, passive: { ...passive, backoff } } }), 1)}{numberField(passive.backoff || {}, 'multiplier', (backoff) => patch({ healthCheck: { ...health, passive: { ...passive, backoff } } }), 1)}{numberField(passive.backoff || {}, 'maxSec', (backoff) => patch({ healthCheck: { ...health, passive: { ...passive, backoff } } }), 1)}</Space></Card>
    </Space></Card>

    <Card size="small" title={t('externalAuth.success')}><Space direction="vertical" style={{ width: '100%' }}>
      <Form.Item label={label(t, 'statusCodes')} style={{ marginBottom: 0 }}><StringList value={(success.statusCodes || []).map(String)} onChange={(codes) => patch({ success: { ...success, statusCodes: codes.map(Number) } })} disabled={disabled} /></Form.Item>
      {(success.body || []).map((predicate: any, index: number) => {
        const op = ['equals','notEquals','exists','in'].find((key) => Object.prototype.hasOwnProperty.call(predicate, key)) || 'equals'
        const updatePredicateValue = (nextValue: unknown) => {
          const body = [...success.body]
          body[index] = { ...predicate, [op]: nextValue }
          patch({ success: { ...success, body } })
        }
        return <Space key={index} wrap><Input value={predicate.pointer || ''} placeholder="pointer" onChange={(e) => { const body = [...success.body]; body[index] = { ...predicate, pointer: e.target.value }; patch({ success: { ...success, body } }) }} disabled={disabled} /><Select value={op} options={['equals','notEquals','exists','in'].map((v) => ({ value: v }))} onChange={(nextOp) => { const body = [...success.body]; body[index] = { pointer: predicate.pointer || '', [nextOp]: nextOp === 'exists' ? true : nextOp === 'in' ? [] : null }; patch({ success: { ...success, body } }) }} disabled={disabled} />{op === 'exists' ? <Checkbox checked={predicate.exists} onChange={(e) => { const body = [...success.body]; body[index] = { ...predicate, exists: e.target.checked }; patch({ success: { ...success, body } }) }} disabled={disabled}>{label(t, 'exists')}</Checkbox> : <JsonValueInput value={predicate[op]} onChange={updatePredicateValue} disabled={disabled} ariaLabel={`predicate-${index}-${op}-json-value`} validate={op === 'in' ? Array.isArray : undefined} />}{!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} onClick={() => patch({ success: { ...success, body: success.body.filter((_: unknown, i: number) => i !== index) } })} />}</Space>
      })}
      {!disabled && <Button type="dashed" icon={<PlusOutlined />} onClick={() => patch({ success: { ...success, body: [...(success.body || []), { pointer: '', equals: '' }] } })}>{t('externalAuth.addPredicate')}</Button>}
    </Space></Card>

    <Card size="small" title={t('externalAuth.request')}><Space direction="vertical" style={{ width: '100%' }}>
      <Space wrap>{textField((request.path || {}) as any, 'template', (path) => patch({ request: { ...request, path } }))}<Checkbox checked={(request.path as any)?.allowOverride ?? false} onChange={(e) => patch({ request: { ...request, path: { ...(request.path as any), allowOverride: e.target.checked } } })} disabled={disabled}>{label(t, 'path.allowOverride')}</Checkbox>{textField((request.method || {}) as any, 'template', (method) => patch({ request: { ...request, method } }))}</Space>
      {(['args','headers','cookies'] as const).map((field) => <Card key={field} size="small" title={field}><ListFieldsEditor value={(request as any)[field]} onChange={(next) => patch({ request: { ...request, [field]: next } })} disabled={disabled} allowForwardAll={field === 'headers'} /></Card>)}
      <Card size="small" title="body"><Space direction="vertical" style={{ width: '100%' }}><Select allowClear value={(request.body as any)?.type} options={['json','form','raw'].map((v) => ({ value: v }))} onChange={(type) => patch({ request: { ...request, body: type === 'form' ? { type, fields: [] } : type ? { type, template: '' } : undefined } })} disabled={disabled} />{(request.body as any)?.type === 'form' ? <ListFieldsEditor value={{ custom: (request.body as any).fields || [] }} onChange={(next) => patch({ request: { ...request, body: { ...(request.body as any), fields: next.custom } } })} disabled={disabled} allowForwardAll={false} /> : (request.body as any)?.type && <Input.TextArea value={(request.body as any).template || ''} onChange={(e) => patch({ request: { ...request, body: { ...(request.body as any), template: e.target.value } } })} disabled={disabled} rows={3} />}</Space></Card>
    </Space></Card>

    <Card size="small" title={t('externalAuth.decision')}><Space wrap>
      <Form.Item label={label(t, 'upstreamHeaders')} style={{ marginBottom: 0 }}><StringList value={decision.upstreamHeaders} onChange={(upstreamHeaders) => patch({ decision: { ...decision, upstreamHeaders } })} disabled={disabled} /></Form.Item>
      <Form.Item label={label(t, 'clientHeaders')} style={{ marginBottom: 0 }}><StringList value={decision.clientHeaders} onChange={(clientHeaders) => patch({ decision: { ...decision, clientHeaders } })} disabled={disabled} /></Form.Item>
      <Checkbox checked={decision.hideCredentials ?? false} onChange={(e) => patch({ decision: { ...decision, hideCredentials: e.target.checked } })} disabled={disabled}>{label(t, 'hideCredentials')}</Checkbox>
      {numberField(decision as any, 'authFailureDelayMs', (next) => patch({ decision: next }), 0)}
    </Space></Card>
  </Space>
}

export default ExternalAuthFilterEditor
