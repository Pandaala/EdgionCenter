/**
 * GatewayClass 表单
 */

import React from 'react'
import { Form, Input, Card, Space, Switch, Tag } from 'antd'
import type { GatewayClass } from '@/utils/gatewayclass'
import { useT } from '@/i18n'

const { TextArea } = Input

interface GatewayClassFormProps {
  data: GatewayClass
  onChange: (data: GatewayClass) => void
  readOnly?: boolean
  isCreate?: boolean
}

const GatewayClassForm: React.FC<GatewayClassFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()

  const updateMeta = (partial: Partial<typeof data.metadata>) =>
    onChange({ ...data, metadata: { ...data.metadata, ...partial } })

  const updateSpec = (partial: Partial<typeof data.spec>) =>
    onChange({ ...data, spec: { ...data.spec, ...partial } })

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <Card title={t('section.basicInfo')} size="small">
          <Form.Item label={t('field.name')} required style={{ marginBottom: 0 }}>
            <Input
              value={data.metadata?.name || ''}
              onChange={(e) => updateMeta({ name: e.target.value })}
              placeholder="edgion"
              disabled={readOnly || !isCreate}
            />
          </Form.Item>
        </Card>

        <Card title={t('section.controllerInfo')} size="small">
          <Form.Item label={t('field.controllerName')} required style={{ marginBottom: 8 }}>
            <Input
              value={data.spec?.controllerName || ''}
              onChange={(e) => updateSpec({ controllerName: e.target.value })}
              placeholder="edgion.io/gateway-controller"
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item label={t('field.descriptionOpt')} style={{ marginBottom: 0 }}>
            <TextArea
              value={data.spec?.description || ''}
              onChange={(e) => updateSpec({ description: e.target.value })}
              placeholder="Edgion Gateway Controller"
              rows={3}
              disabled={readOnly}
            />
          </Form.Item>
        </Card>

        <Card title={t('section.parametersRef')} size="small">
          <Form.Item label={t('field.parametersRefEnabled')} style={{ marginBottom: 8 }}>
            <Switch
              checked={!!data.spec.parametersRef}
              disabled={readOnly}
              onChange={(checked) => updateSpec({ parametersRef: checked ? { group: 'edgion.io', kind: 'EdgionGatewayConfig', name: '' } : undefined })}
            />
          </Form.Item>
          {data.spec.parametersRef && <Space wrap>
            <Form.Item label={t('field.group')} style={{ marginBottom: 0 }}><Input value={data.spec.parametersRef.group} disabled={readOnly} onChange={(event) => updateSpec({ parametersRef: { ...data.spec.parametersRef!, group: event.target.value } })} /></Form.Item>
            <Form.Item label={t('field.kind')} style={{ marginBottom: 0 }}><Input value={data.spec.parametersRef.kind} disabled={readOnly} onChange={(event) => updateSpec({ parametersRef: { ...data.spec.parametersRef!, kind: event.target.value } })} /></Form.Item>
            <Form.Item label={t('field.name')} required style={{ marginBottom: 0 }}><Input value={data.spec.parametersRef.name} disabled={readOnly} onChange={(event) => updateSpec({ parametersRef: { ...data.spec.parametersRef!, name: event.target.value } })} /></Form.Item>
          </Space>}
        </Card>

        {Array.isArray(data.status?.supportedFeatures) && <Card title={t('section.supportedFeatures')} size="small">
          <Space wrap>{data.status.supportedFeatures.map((feature: any, index: number) => <Tag key={`${String(feature?.name || feature)}-${index}`}>{String(feature?.name || feature)}</Tag>)}</Space>
        </Card>}
      </Space>
    </Form>
  )
}

export default GatewayClassForm
