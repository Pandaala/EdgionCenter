/**
 * StreamRoute 表单 — 共享用于 TCPRoute / UDPRoute / TLSRoute
 * TCPRoute: parentRefs + annotations + backendRefs
 * UDPRoute: parentRefs + backendRefs
 * TLSRoute: parentRefs + hostnames + backendRefs + annotations
 */

import React, { useEffect, useState } from 'react'
import { Button, Card, Form, Select, Space } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import MetadataSection from '../common/MetadataSection'
import ParentRefsSection from '../common/ParentRefsSection'
import HostnamesSection from '../common/HostnamesSection'
import BackendRefsEditor from '../common/BackendRefsEditor'
import StreamAnnotationsSection from './StreamAnnotationsSection'
import { useT } from '@/i18n'

export type StreamRouteKind = 'TCPRoute' | 'UDPRoute' | 'TLSRoute'

interface StreamRouteFormProps {
  kind: StreamRouteKind
  data: any
  onChange: (data: any) => void
  readOnly?: boolean
  isCreate?: boolean
}

export function replaceRuleBackendRefs<T extends { spec?: { rules?: unknown[] } }>(
  resource: T,
  ruleIndex: number,
  backendRefs: unknown[],
): T {
  const rules = [...(resource.spec?.rules || [])] as Array<Record<string, unknown>>
  const currentRule = rules[ruleIndex] || {}
  rules[ruleIndex] = { ...currentRule, backendRefs }
  return {
    ...resource,
    spec: { ...resource.spec, rules },
  }
}

const StreamRouteForm: React.FC<StreamRouteFormProps> = ({
  kind,
  data,
  onChange,
  readOnly = false,
  isCreate = true,
}) => {
  const t = useT()
  const rules = data.spec?.rules || []
  const [selectedRule, setSelectedRule] = useState(0)

  useEffect(() => {
    if (selectedRule >= rules.length) setSelectedRule(Math.max(0, rules.length - 1))
  }, [rules.length, selectedRule])

  const update = (path: string, value: any) => {
    if (path.startsWith('metadata.')) {
      const field = path.slice('metadata.'.length)
      onChange({ ...data, metadata: { ...data.metadata, [field]: value } })
    } else if (path.startsWith('spec.')) {
      const field = path.slice('spec.'.length)
      onChange({ ...data, spec: { ...data.spec, [field]: value } })
    } else if (path === 'metadata') {
      onChange({ ...data, metadata: value })
    } else if (path === 'spec') {
      onChange({ ...data, spec: value })
    }
  }

  const handleRulesChange = (backendRefs: any[]) => {
    onChange(replaceRuleBackendRefs(data, selectedRule, backendRefs))
  }

  const addRule = () => {
    const nextRules = [...rules, { backendRefs: [{ name: '', port: kind === 'TLSRoute' ? 443 : 80, weight: 1 }] }]
    onChange({ ...data, spec: { ...data.spec, rules: nextRules } })
    setSelectedRule(nextRules.length - 1)
  }

  const removeRule = () => {
    const nextRules = rules.filter((_: unknown, index: number) => index !== selectedRule)
    onChange({ ...data, spec: { ...data.spec, rules: nextRules } })
    setSelectedRule(Math.max(0, selectedRule - 1))
  }

  const backendRefs = rules[selectedRule]?.backendRefs || []

  const showHostnames = kind === 'TLSRoute'
  const showAnnotations = true

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(meta) => update('metadata', meta)}
          disabled={readOnly}
          isCreate={isCreate}
        />

        {showAnnotations && (
          <StreamAnnotationsSection
            kind={kind}
            annotations={data.metadata?.annotations || {}}
            onChange={(annotations) =>
              onChange({ ...data, metadata: { ...data.metadata, annotations } })
            }
            disabled={readOnly}
          />
        )}

        <ParentRefsSection
          value={data.spec?.parentRefs || []}
          onChange={(refs) => update('spec', { ...data.spec, parentRefs: refs })}
          disabled={readOnly}
          namespace={data.metadata?.namespace}
        />

        {showHostnames && (
          <HostnamesSection
            value={data.spec?.hostnames || []}
            onChange={(hostnames) => update('spec', { ...data.spec, hostnames })}
            disabled={readOnly}
          />
        )}

        <Card title={t('routeRule.rules')} size="small">
          <Space direction="vertical" style={{ width: '100%' }}>
          {rules.length > 0 && (
          <Form.Item label={t('routeRule.selected')} style={{ marginBottom: 0 }}>
            <Select
              aria-label={t('routeRule.selected')}
              value={selectedRule}
              onChange={setSelectedRule}
              disabled={readOnly}
              options={rules.map((_: any, index: number) => ({
                value: index,
                label: t('routeRule.number', { n: index + 1 }),
              }))}
            />
          </Form.Item>
          )}

          {rules.length > 0 && <BackendRefsEditor
            value={backendRefs}
            onChange={handleRulesChange}
            disabled={readOnly}
            namespace={data.metadata?.namespace}
          />}
          {!readOnly && <Space>
            <Button data-testid="streamroute-rule-add" type="dashed" icon={<PlusOutlined />} onClick={addRule}>{t('btn.addRule')}</Button>
            {rules.length > 0 && <Button data-testid="streamroute-rule-remove" danger icon={<MinusCircleOutlined />} onClick={removeRule}>{t('routeRule.remove')}</Button>}
          </Space>}
          </Space>
        </Card>
      </Space>
    </Form>
  )
}

export default StreamRouteForm
