import React from 'react'
import { Button, Card, Form, Input, InputNumber, Space } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import type { GRPCBackendRef, GRPCRouteFilter } from '@/types/gateway-api/grpcroute'
import RouteFiltersEditor from '../../HTTPRoute/rule-components/RouteFiltersEditor'
import { useT } from '@/i18n'

interface Props {
  value?: GRPCBackendRef[]
  onChange: (value: GRPCBackendRef[]) => void
  disabled?: boolean
  namespace?: string
}

const GRPCBackendRefsEditor: React.FC<Props> = ({ value = [], onChange, disabled = false, namespace = 'default' }) => {
  const t = useT()
  const update = (index: number, backend: GRPCBackendRef) => {
    const next = [...value]
    next[index] = backend
    onChange(next)
  }
  return <Space direction="vertical" style={{ width: '100%' }}>
    {value.map((backend, index) => <Card key={index} size="small" title={t('grpc.backend', { n: index + 1 })} extra={!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} onClick={() => onChange(value.filter((_, i) => i !== index))} />}>
      <Space direction="vertical" style={{ width: '100%' }}>
        <Form.Item label={t('field.name')} style={{ marginBottom: 0 }}><Input value={backend.name} onChange={(e) => update(index, { ...backend, name: e.target.value })} disabled={disabled} /></Form.Item>
        <Space wrap>
          <Form.Item label={t('field.namespaceOpt')} style={{ marginBottom: 0 }}><Input value={backend.namespace || ''} placeholder={namespace} onChange={(e) => update(index, { ...backend, namespace: e.target.value })} disabled={disabled} /></Form.Item>
          <Form.Item label={t('field.portOpt')} style={{ marginBottom: 0 }}><InputNumber value={backend.port} min={1} max={65535} onChange={(port) => update(index, { ...backend, port: port ?? undefined })} disabled={disabled} /></Form.Item>
          <Form.Item label={t('field.weight')} style={{ marginBottom: 0 }}><InputNumber value={backend.weight} min={0} onChange={(weight) => update(index, { ...backend, weight: weight ?? undefined })} disabled={disabled} /></Form.Item>
          <Form.Item label={t('field.group')} style={{ marginBottom: 0 }}><Input value={backend.group || ''} onChange={(e) => update(index, { ...backend, group: e.target.value })} disabled={disabled} /></Form.Item>
          <Form.Item label={t('field.kind')} style={{ marginBottom: 0 }}><Input value={backend.kind || ''} onChange={(e) => update(index, { ...backend, kind: e.target.value })} disabled={disabled} /></Form.Item>
        </Space>
        <Card size="small" title={t('routeFilter.backendFilters')}><RouteFiltersEditor value={backend.filters} onChange={(filters) => update(index, { ...backend, filters: filters as GRPCRouteFilter[] })} disabled={disabled} protocol="grpc" /></Card>
      </Space>
    </Card>)}
    {!disabled && <Button type="dashed" icon={<PlusOutlined />} onClick={() => onChange([...value, { name: '', namespace, port: 50051, weight: 1 }])}>{t('grpc.addBackend')}</Button>}
  </Space>
}

export default GRPCBackendRefsEditor
