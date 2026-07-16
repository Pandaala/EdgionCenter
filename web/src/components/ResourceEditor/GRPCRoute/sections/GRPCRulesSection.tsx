import React from 'react'
import { Button, Card, Collapse, Form, Input, Space } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import GRPCMethodMatchEditor from './GRPCMethodMatchEditor'
import GRPCBackendRefsEditor from './GRPCBackendRefsEditor'
import RouteFiltersEditor from '../../HTTPRoute/rule-components/RouteFiltersEditor'
import RulePoliciesEditor from '../../HTTPRoute/rule-components/RulePoliciesEditor'
import type { GRPCRouteFilter, GRPCRouteRule } from '@/types/gateway-api/grpcroute'
import { useT } from '@/i18n'

interface Props {
  value?: GRPCRouteRule[]
  onChange?: (value: GRPCRouteRule[]) => void
  disabled?: boolean
  namespace?: string
}

const defaultRule = (namespace: string): GRPCRouteRule => ({
  matches: [{ method: { type: 'Exact', service: '', method: '' } }],
  backendRefs: [{ name: '', namespace, port: 50051, weight: 1 }],
})

const GRPCRulesSection: React.FC<Props> = ({ value = [], onChange, disabled = false, namespace = 'default' }) => {
  const t = useT()
  const update = (index: number, rule: GRPCRouteRule) => {
    const next = [...value]
    next[index] = rule
    onChange?.(next)
  }
  const items = value.map((rule, index) => ({
    key: String(index),
    label: rule.name || t('routeRule.number', { n: index + 1 }),
    extra: !disabled && <Button data-testid="grpcroute-rule-remove" danger size="small" icon={<MinusCircleOutlined />} onClick={(event) => { event.stopPropagation(); onChange?.(value.filter((_, i) => i !== index)) }}>{t('btn.delete')}</Button>,
    children: <Space direction="vertical" style={{ width: '100%' }} size="middle">
      <Form.Item label={t('field.ruleName')} style={{ marginBottom: 0 }}><Input value={rule.name || ''} onChange={(event) => update(index, { ...rule, name: event.target.value })} disabled={disabled} /></Form.Item>
      <Card title={t('grpc.matchConditions')} size="small"><GRPCMethodMatchEditor value={rule.matches || []} onChange={(matches) => update(index, { ...rule, matches })} disabled={disabled} /></Card>
      <Card title={t('routeRule.filters')} size="small"><RouteFiltersEditor value={rule.filters} onChange={(filters) => update(index, { ...rule, filters: filters as GRPCRouteFilter[] })} disabled={disabled} protocol="grpc" /></Card>
      <Card title={t('routeRule.backends')} size="small"><GRPCBackendRefsEditor value={rule.backendRefs} onChange={(backendRefs) => update(index, { ...rule, backendRefs })} disabled={disabled} namespace={namespace} /></Card>
      <RulePoliciesEditor value={rule} onChange={(next) => update(index, next as GRPCRouteRule)} disabled={disabled} protocol="grpc" />
    </Space>,
  }))
  return <div>
    <Collapse items={items} defaultActiveKey={value.length ? ['0'] : []} />
    {!disabled && <Button data-testid="grpcroute-rule-add" type="dashed" onClick={() => onChange?.([...value, defaultRule(namespace)])} block icon={<PlusOutlined />} style={{ marginTop: 12 }}>{t('btn.addRule')}</Button>}
  </div>
}

export default GRPCRulesSection
