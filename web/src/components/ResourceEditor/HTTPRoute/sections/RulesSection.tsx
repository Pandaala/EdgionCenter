import React from 'react'
import { Button, Card, Collapse, Form, Input, Space, Tag, Typography } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import MatchesEditor from '../rule-components/MatchesEditor'
import BackendRefsEditor from '../rule-components/BackendRefsEditor'
import RouteFiltersEditor from '../rule-components/RouteFiltersEditor'
import RulePoliciesEditor from '../rule-components/RulePoliciesEditor'
import { DEFAULT_VALUES } from '@/constants/gateway-api'
import type { HTTPRouteRule } from '@/types/gateway-api'
import type { HTTPRouteFilter } from '@/types/gateway-api/httproute'
import { useT } from '@/i18n'

interface Props {
  value?: HTTPRouteRule[]
  onChange?: (value: HTTPRouteRule[]) => void
  disabled?: boolean
  namespace?: string
}

const defaultRule = (namespace: string): HTTPRouteRule => ({
  matches: [{ path: { type: 'PathPrefix', value: '/' } }],
  backendRefs: [{ group: '', kind: 'Service', namespace, name: '', port: 80, weight: 1 }],
})

const RulesSection: React.FC<Props> = ({ value = [], onChange, disabled = false, namespace = DEFAULT_VALUES.defaultNamespace }) => {
  const t = useT()
  const update = (index: number, rule: HTTPRouteRule) => {
    const next = [...value]
    next[index] = rule
    onChange?.(next)
  }
  const items = value.map((rule, index) => {
    const path = rule.matches?.[0]?.path
    return {
      key: String(index),
      label: <Space><Typography.Text strong>{rule.name || t('routeRule.number', { n: index + 1 })}</Typography.Text>{path && <><Tag color="blue">{path.type || 'PathPrefix'}</Tag><Typography.Text code>{path.value || '/'}</Typography.Text></>}</Space>,
      extra: !disabled && <Button data-testid="httproute-rule-remove" danger size="small" icon={<MinusCircleOutlined />} onClick={(event) => { event.stopPropagation(); onChange?.(value.filter((_, i) => i !== index)) }}>{t('btn.delete')}</Button>,
      children: <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <Form.Item label={t('field.ruleName')} style={{ marginBottom: 0 }}><Input value={rule.name || ''} onChange={(event) => update(index, { ...rule, name: event.target.value })} disabled={disabled} /></Form.Item>
        <Card size="small" title={t('routeRule.matches')}><MatchesEditor value={rule.matches} onChange={(matches) => update(index, { ...rule, matches })} disabled={disabled} /></Card>
        <Card size="small" title={t('routeRule.filters')}><RouteFiltersEditor value={rule.filters} onChange={(filters) => update(index, { ...rule, filters: filters as HTTPRouteFilter[] })} disabled={disabled} protocol="http" /></Card>
        <Card size="small" title={t('routeRule.backends')}><BackendRefsEditor value={rule.backendRefs} onChange={(backendRefs) => update(index, { ...rule, backendRefs })} disabled={disabled} namespace={namespace} protocol="http" /></Card>
        <RulePoliciesEditor value={rule} onChange={(next) => update(index, next as HTTPRouteRule)} disabled={disabled} protocol="http" />
      </Space>,
    }
  })
  return <Card title={t('routeRule.rules')} size="small">
    <Collapse items={items} defaultActiveKey={value.length ? ['0'] : []} />
    {!disabled && <Button data-testid="httproute-rule-add" type="dashed" block icon={<PlusOutlined />} style={{ marginTop: 12 }} onClick={() => onChange?.([...value, defaultRule(namespace)])}>{t('btn.addRule')}</Button>}
  </Card>
}

export default RulesSection
