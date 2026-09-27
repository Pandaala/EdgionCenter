import React from 'react'
import { Alert, Button, Card, Checkbox, Form, Input, InputNumber, Select, Space } from 'antd'
import type { HTTPRouteRule } from '@/types/gateway-api/httproute'
import type { GRPCRouteRule } from '@/types/gateway-api/grpcroute'
import { useT } from '@/i18n'

interface Props {
  value: HTTPRouteRule | GRPCRouteRule
  onChange: (value: HTTPRouteRule | GRPCRouteRule) => void
  disabled?: boolean
  protocol?: 'http' | 'grpc'
}

const RulePoliciesEditor: React.FC<Props> = ({ value, onChange, disabled = false, protocol = 'http' }) => {
  const t = useT()
  const timeouts = value.timeouts || {}
  const retry = value.retry || {}
  const session = value.sessionPersistence || {}
  const updateText = (section: 'timeouts' | 'retry' | 'sessionPersistence', key: string, text: string) => {
    const next: Record<string, unknown> = { ...value[section] }
    if (text === '') delete next[key]
    else next[key] = text
    onChange({ ...value, [section]: next })
  }
  return <Space direction="vertical" style={{ width: '100%' }}>
    <Card size="small" title={t('routePolicy.timeouts')}>
      <Space wrap>
        <Form.Item label={t('routePolicy.request')} style={{ marginBottom: 0 }}><Input aria-label={t('routePolicy.request')} value={timeouts.request || ''} onChange={(e) => updateText('timeouts', 'request', e.target.value)} disabled={disabled} placeholder="30s" /></Form.Item>
        <Form.Item label={t('routePolicy.backendRequest')} style={{ marginBottom: 0 }}><Input aria-label={t('routePolicy.backendRequest')} value={timeouts.backendRequest || ''} onChange={(e) => updateText('timeouts', 'backendRequest', e.target.value)} disabled={disabled} placeholder="10s" /></Form.Item>
      </Space>
    </Card>
    <Card size="small" title={t('routePolicy.retry')}>
      <Space wrap>
        <Form.Item label={t('routePolicy.attempts')} style={{ marginBottom: 0 }}><InputNumber aria-label={t('routePolicy.attempts')} min={protocol === 'grpc' ? 0 : 1} precision={0} value={retry.attempts} onChange={(attempts) => onChange({ ...value, retry: { ...retry, attempts: attempts ?? undefined } })} disabled={disabled} /></Form.Item>
        <Form.Item label={t('routePolicy.backoff')} style={{ marginBottom: 0 }}><Input aria-label={t('routePolicy.backoff')} value={retry.backoff || ''} onChange={(e) => updateText('retry', 'backoff', e.target.value)} disabled={disabled} placeholder="1s" /></Form.Item>
        <Form.Item label={protocol === 'grpc' ? t('routePolicy.grpcCodes') : t('routePolicy.httpCodes')} help={protocol === 'grpc' ? t('routePolicy.grpcCodesIgnored') : undefined} style={{ marginBottom: 0 }}><Select aria-label={protocol === 'grpc' ? t('routePolicy.grpcCodes') : t('routePolicy.httpCodes')} mode="tags" value={(retry.codes || []).map(String)} onChange={(codes) => {
          const min = protocol === 'grpc' ? 0 : 400
          const max = protocol === 'grpc' ? 16 : 599
          onChange({ ...value, retry: { ...retry, codes: codes.map(Number).filter((code) => Number.isInteger(code) && code >= min && code <= max) } })
        }} disabled={disabled} style={{ minWidth: 240 }} /></Form.Item>
      </Space>
    </Card>
    <Card size="small" title={t('routePolicy.session')}>
      <Space wrap>
        <Form.Item label={t('routePolicy.sessionName')} style={{ marginBottom: 0 }}><Input aria-label={t('routePolicy.sessionName')} value={session.sessionName || ''} onChange={(e) => updateText('sessionPersistence', 'sessionName', e.target.value)} disabled={disabled} /></Form.Item>
        <Form.Item label={t('routePolicy.type')} style={{ marginBottom: 0 }}><Select allowClear value={session.type} options={['Cookie','Header'].map((v) => ({ value: v }))} onChange={(type) => onChange({ ...value, sessionPersistence: { ...session, type } })} disabled={disabled} style={{ width: 120 }} /></Form.Item>
        <Form.Item label={t('routePolicy.absoluteTimeout')} style={{ marginBottom: 0 }}><Input aria-label={t('routePolicy.absoluteTimeout')} value={session.absoluteTimeout || ''} onChange={(e) => updateText('sessionPersistence', 'absoluteTimeout', e.target.value)} disabled={disabled} /></Form.Item>
        {session.idleTimeout !== undefined && <Alert type="warning" message={t('routePolicy.idleUnsupported')}
          action={!disabled && <Button size="small" onClick={() => updateText('sessionPersistence', 'idleTimeout', '')}>{t('btn.clear')}</Button>} />}
        {session.type !== 'Header' && <Form.Item label={t('routePolicy.lifetimeType')} style={{ marginBottom: 0 }}><Select allowClear value={session.cookieConfig?.lifetimeType} options={['Permanent','Session'].map((v) => ({ value: v }))} onChange={(lifetimeType) => onChange({ ...value, sessionPersistence: { ...session, cookieConfig: { ...session.cookieConfig, lifetimeType } } })} disabled={disabled} style={{ width: 140 }} /></Form.Item>}
        <Checkbox checked={session.strict ?? false} onChange={(e) => onChange({ ...value, sessionPersistence: { ...session, strict: e.target.checked } })} disabled={disabled}>{t('routePolicy.strict')}</Checkbox>
        <Alert type="info" message={t('routePolicy.strictScope')} />
      </Space>
    </Card>
  </Space>
}

export default RulePoliciesEditor
