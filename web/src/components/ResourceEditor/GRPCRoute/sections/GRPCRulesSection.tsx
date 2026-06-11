/**
 * GRPCRoute Rules 区段 — 编辑 rules 数组
 */

import React from 'react'
import { Card, Form, Input, InputNumber, Button, Space, Collapse } from 'antd'
import { PlusOutlined, MinusCircleOutlined } from '@ant-design/icons'
import GRPCMethodMatchEditor from './GRPCMethodMatchEditor'
import type { GRPCRouteRule } from '@/types/gateway-api/grpcroute'
import type { BackendRef } from '@/types/gateway-api/backend'
import { PORT_MIN, PORT_MAX, WEIGHT_MIN, WEIGHT_MAX } from '@/constants/gateway-api'
import { useT } from '@/i18n'

interface GRPCRulesSectionProps {
  value?: GRPCRouteRule[]
  onChange?: (value: GRPCRouteRule[]) => void
  disabled?: boolean
  namespace?: string
}

const defaultRule = (): GRPCRouteRule => ({
  matches: [{ method: { type: 'Exact', service: '', method: '' } }],
  backendRefs: [{ name: '', port: 50051, weight: 1 }],
})

const GRPCRulesSection: React.FC<GRPCRulesSectionProps> = ({
  value = [],
  onChange,
  disabled = false,
  namespace = 'default',
}) => {
  const t = useT()

  const updateRule = (index: number, rule: GRPCRouteRule) => {
    const next = [...value]
    next[index] = rule
    onChange?.(next)
  }

  const addRule = () => onChange?.([...value, defaultRule()])

  const removeRule = (index: number) => onChange?.(value.filter((_, i) => i !== index))

  const updateBackend = (ruleIndex: number, backendIndex: number, backend: BackendRef) => {
    const rule = value[ruleIndex]
    const backendRefs = [...(rule.backendRefs || [])]
    backendRefs[backendIndex] = backend
    updateRule(ruleIndex, { ...rule, backendRefs })
  }

  const addBackend = (ruleIndex: number) => {
    const rule = value[ruleIndex]
    updateRule(ruleIndex, {
      ...rule,
      backendRefs: [
        ...(rule.backendRefs || []),
        { name: '', port: 50051, weight: 1, namespace },
      ],
    })
  }

  const removeBackend = (ruleIndex: number, backendIndex: number) => {
    const rule = value[ruleIndex]
    updateRule(ruleIndex, {
      ...rule,
      backendRefs: (rule.backendRefs || []).filter((_, i) => i !== backendIndex),
    })
  }

  const items = value.map((rule, ruleIndex) => ({
    key: String(ruleIndex),
    label: rule.name ? t('grpc.namedRule', { name: rule.name }) : t('grpc.rule', { n: ruleIndex + 1 }),
    extra: !disabled && value.length > 1 && (
      <Button danger size="small" icon={<MinusCircleOutlined />}
        onClick={(e) => { e.stopPropagation(); removeRule(ruleIndex) }}>{t('btn.delete')}</Button>
    ),
    children: (
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <Form.Item label={t('field.ruleName')} style={{ marginBottom: 0 }}>
          <Input
            value={rule.name || ''}
            onChange={(e) => updateRule(ruleIndex, { ...rule, name: e.target.value })}
            placeholder="my-rule"
            disabled={disabled}
            style={{ width: 260 }}
          />
        </Form.Item>

        <Card title={t('grpc.matchConditions')} size="small">
          <GRPCMethodMatchEditor
            value={rule.matches || []}
            onChange={(matches) => updateRule(ruleIndex, { ...rule, matches })}
            disabled={disabled}
          />
        </Card>

        <Card title={t('grpc.backendService')} size="small">
          {(rule.backendRefs || []).map((backend, bIdx) => (
            <Card key={bIdx} type="inner" size="small"
              title={t('grpc.backend', { n: bIdx + 1 })}
              extra={!disabled && (rule.backendRefs || []).length > 1 && (
                <Button danger size="small" icon={<MinusCircleOutlined />}
                  onClick={() => removeBackend(ruleIndex, bIdx)}>{t('btn.delete')}</Button>
              )}
              style={{ marginBottom: 8 }}
            >
              <Space direction="vertical" style={{ width: '100%' }}>
                <Form.Item label={t('field.name')} required style={{ marginBottom: 0 }}>
                  <Input
                    value={backend.name}
                    onChange={(e) => updateBackend(ruleIndex, bIdx, { ...backend, name: e.target.value })}
                    placeholder="grpc-service"
                    disabled={disabled}
                  />
                </Form.Item>
                <Form.Item label={t('field.port')} style={{ marginBottom: 0 }}>
                  <InputNumber
                    value={backend.port}
                    onChange={(v) => updateBackend(ruleIndex, bIdx, { ...backend, port: v || undefined })}
                    min={PORT_MIN} max={PORT_MAX}
                    disabled={disabled}
                    style={{ width: 120 }}
                    placeholder="50051"
                  />
                </Form.Item>
                <Form.Item label={t('field.weight')} style={{ marginBottom: 0 }}>
                  <InputNumber
                    value={backend.weight ?? 1}
                    onChange={(v) => updateBackend(ruleIndex, bIdx, { ...backend, weight: v ?? 1 })}
                    min={WEIGHT_MIN} max={WEIGHT_MAX}
                    disabled={disabled}
                    style={{ width: 120 }}
                  />
                </Form.Item>
              </Space>
            </Card>
          ))}
          {!disabled && (
            <Button type="dashed" block icon={<PlusOutlined />} onClick={() => addBackend(ruleIndex)}>
              {t('grpc.addBackend')}
            </Button>
          )}
        </Card>
      </Space>
    ),
  }))

  return (
    <div>
      <Collapse items={items} defaultActiveKey={['0']} />
      {!disabled && (
        <Button type="dashed" onClick={addRule} block icon={<PlusOutlined />} style={{ marginTop: 12 }}>
          {t('btn.addRule')}
        </Button>
      )}
    </div>
  )
}

export default GRPCRulesSection
