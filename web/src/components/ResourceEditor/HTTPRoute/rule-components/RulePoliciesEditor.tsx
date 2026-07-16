import React from 'react'
import { Card, Checkbox, Form, Input, InputNumber, Select, Space } from 'antd'
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
  return <Space direction="vertical" style={{ width: '100%' }}>
    <Card size="small" title={t('routePolicy.timeouts')}>
      <Space wrap>
        <Form.Item label={t('routePolicy.request')} style={{ marginBottom: 0 }}><Input value={timeouts.request || ''} onChange={(e) => onChange({ ...value, timeouts: { ...timeouts, request: e.target.value } })} disabled={disabled} placeholder="30s" /></Form.Item>
        <Form.Item label={t('routePolicy.backendRequest')} style={{ marginBottom: 0 }}><Input value={timeouts.backendRequest || ''} onChange={(e) => onChange({ ...value, timeouts: { ...timeouts, backendRequest: e.target.value } })} disabled={disabled} placeholder="10s" /></Form.Item>
      </Space>
    </Card>
    <Card size="small" title={t('routePolicy.retry')}>
      <Space wrap>
        <Form.Item label={t('routePolicy.attempts')} style={{ marginBottom: 0 }}><InputNumber min={0} value={retry.attempts} onChange={(attempts) => onChange({ ...value, retry: { ...retry, attempts: attempts ?? undefined } })} disabled={disabled} /></Form.Item>
        <Form.Item label={t('routePolicy.backoff')} style={{ marginBottom: 0 }}><Input value={retry.backoff || ''} onChange={(e) => onChange({ ...value, retry: { ...retry, backoff: e.target.value } })} disabled={disabled} placeholder="1s" /></Form.Item>
        <Form.Item label={protocol === 'grpc' ? t('routePolicy.grpcCodes') : t('routePolicy.httpCodes')} style={{ marginBottom: 0 }}><Select mode="tags" value={(retry.codes || []).map(String)} onChange={(codes) => {
          const min = protocol === 'grpc' ? 0 : 100
          const max = protocol === 'grpc' ? 16 : 599
          onChange({ ...value, retry: { ...retry, codes: codes.map(Number).filter((code) => Number.isInteger(code) && code >= min && code <= max) } })
        }} disabled={disabled} style={{ minWidth: 240 }} /></Form.Item>
      </Space>
    </Card>
    <Card size="small" title={t('routePolicy.session')}>
      <Space wrap>
        <Form.Item label={t('routePolicy.sessionName')} style={{ marginBottom: 0 }}><Input value={session.sessionName || ''} onChange={(e) => onChange({ ...value, sessionPersistence: { ...session, sessionName: e.target.value } })} disabled={disabled} /></Form.Item>
        <Form.Item label={t('routePolicy.type')} style={{ marginBottom: 0 }}><Select allowClear value={session.type} options={['Cookie','Header'].map((v) => ({ value: v }))} onChange={(type) => onChange({ ...value, sessionPersistence: { ...session, type } })} disabled={disabled} style={{ width: 120 }} /></Form.Item>
        <Form.Item label={t('routePolicy.absoluteTimeout')} style={{ marginBottom: 0 }}><Input value={session.absoluteTimeout || ''} onChange={(e) => onChange({ ...value, sessionPersistence: { ...session, absoluteTimeout: e.target.value } })} disabled={disabled} /></Form.Item>
        <Form.Item label={t('routePolicy.idleTimeout')} style={{ marginBottom: 0 }}><Input value={session.idleTimeout || ''} onChange={(e) => onChange({ ...value, sessionPersistence: { ...session, idleTimeout: e.target.value } })} disabled={disabled} /></Form.Item>
        {session.type !== 'Header' && <Form.Item label={t('routePolicy.lifetimeType')} style={{ marginBottom: 0 }}><Select allowClear value={session.cookieConfig?.lifetimeType} options={['Permanent','Session'].map((v) => ({ value: v }))} onChange={(lifetimeType) => onChange({ ...value, sessionPersistence: { ...session, cookieConfig: { ...session.cookieConfig, lifetimeType } } })} disabled={disabled} style={{ width: 140 }} /></Form.Item>}
        <Checkbox checked={session.strict ?? false} onChange={(e) => onChange({ ...value, sessionPersistence: { ...session, strict: e.target.checked } })} disabled={disabled}>{t('routePolicy.strict')}</Checkbox>
      </Space>
    </Card>
  </Space>
}

export default RulePoliciesEditor
