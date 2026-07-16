import React from 'react'
import { Button, Card, Checkbox, Form, Input, InputNumber, Select, Space } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import type {
  HTTPExternalAuthFilter,
  HTTPRequestHeaderFilter,
  HTTPRouteFilter,
} from '@/types/gateway-api/httproute'
import type { HTTPHeader } from '@/types/gateway-api/backend'
import type { GRPCRouteFilter } from '@/types/gateway-api/grpcroute'
import { useT } from '@/i18n'
import ExternalAuthFilterEditor from './ExternalAuthFilterEditor'

export type RouteFilter = HTTPRouteFilter | GRPCRouteFilter

interface Props {
  value?: RouteFilter[]
  onChange?: (value: RouteFilter[]) => void
  disabled?: boolean
  protocol?: 'http' | 'grpc'
}

const HTTP_TYPES = [
  'RequestHeaderModifier',
  'ResponseHeaderModifier',
  'RequestRedirect',
  'URLRewrite',
  'RequestMirror',
  'CORS',
  'ExternalAuth',
  'ExtensionRef',
] as const
const GRPC_TYPES = ['RequestHeaderModifier', 'ResponseHeaderModifier', 'ExtensionRef'] as const

function HeaderRows({
  value = {}, onChange, disabled,
}: {
  value?: HTTPRequestHeaderFilter
  onChange: (value: HTTPRequestHeaderFilter) => void
  disabled: boolean
}) {
  const t = useT()
  const setRows = (key: 'set' | 'add', rows: HTTPHeader[]) => onChange({ ...value, [key]: rows })
  return (
    <Space direction="vertical" style={{ width: '100%' }}>
      {(['set', 'add'] as const).map((key) => (
        <Card key={key} size="small" title={t(`routeFilter.${key}` as any)}>
          {(value[key] || []).map((header, index) => (
            <Space key={index} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
              <Input aria-label={`${key}-name-${index}`} value={header.name}
                onChange={(event) => {
                  const rows = [...(value[key] || [])]
                  rows[index] = { ...header, name: event.target.value }
                  setRows(key, rows)
                }} placeholder={t('field.name')} disabled={disabled} />
              <Input aria-label={`${key}-value-${index}`} value={header.value}
                onChange={(event) => {
                  const rows = [...(value[key] || [])]
                  rows[index] = { ...header, value: event.target.value }
                  setRows(key, rows)
                }} placeholder={t('field.value')} disabled={disabled} />
              {!disabled && <Button type="text" danger icon={<MinusCircleOutlined />}
                onClick={() => setRows(key, (value[key] || []).filter((_, i) => i !== index))} />}
            </Space>
          ))}
          {!disabled && <Button type="dashed" size="small" icon={<PlusOutlined />}
            onClick={() => setRows(key, [...(value[key] || []), { name: '', value: '' }])}>
            {t('routeFilter.addHeader')}
          </Button>}
        </Card>
      ))}
      <Form.Item label={t('routeFilter.remove')} style={{ marginBottom: 0 }}>
        <Select mode="tags" value={value.remove || []} onChange={(remove) => onChange({ ...value, remove })}
          disabled={disabled} tokenSeparators={[',']} />
      </Form.Item>
    </Space>
  )
}

function ObjectRefFields({ value = {}, onChange, disabled }: {
  value?: Record<string, any>
  onChange: (value: Record<string, any>) => void
  disabled: boolean
}) {
  const t = useT()
  return <Space direction="vertical" style={{ width: '100%' }}>
    <Form.Item label={t('field.name')} required style={{ marginBottom: 0 }}>
      <Input value={value.name || ''} onChange={(e) => onChange({ ...value, name: e.target.value })} disabled={disabled} />
    </Form.Item>
    <Space wrap>
      <Form.Item label={t('field.namespaceOpt')} style={{ marginBottom: 0 }}>
        <Input value={value.namespace || ''} onChange={(e) => onChange({ ...value, namespace: e.target.value })} disabled={disabled} />
      </Form.Item>
      <Form.Item label={t('field.portOpt')} style={{ marginBottom: 0 }}>
        <InputNumber value={value.port} min={1} max={65535} onChange={(port) => onChange({ ...value, port: port ?? undefined })} disabled={disabled} />
      </Form.Item>
      <Form.Item label={t('field.group')} style={{ marginBottom: 0 }}>
        <Input value={value.group || ''} onChange={(e) => onChange({ ...value, group: e.target.value })} disabled={disabled} />
      </Form.Item>
      <Form.Item label={t('field.kind')} style={{ marginBottom: 0 }}>
        <Input value={value.kind || ''} onChange={(e) => onChange({ ...value, kind: e.target.value })} disabled={disabled} />
      </Form.Item>
    </Space>
  </Space>
}

function FilterBody({ filter, onChange, disabled }: {
  filter: RouteFilter
  onChange: (filter: RouteFilter) => void
  disabled: boolean
}) {
  const t = useT()
  if (filter.type === 'RequestHeaderModifier' || filter.type === 'ResponseHeaderModifier') {
    const key = filter.type === 'RequestHeaderModifier' ? 'requestHeaderModifier' : 'responseHeaderModifier'
    return <HeaderRows value={(filter as any)[key]} onChange={(next) => onChange({ ...filter, [key]: next })} disabled={disabled} />
  }
  if (filter.type === 'ExtensionRef') {
    const ref = (filter as any).extensionRef || {}
    return <Space wrap>
      <Form.Item label={t('field.group')} style={{ marginBottom: 0 }}><Input value={ref.group || ''} onChange={(e) => onChange({ ...filter, extensionRef: { ...ref, group: e.target.value } })} disabled={disabled} /></Form.Item>
      <Form.Item label={t('field.kind')} style={{ marginBottom: 0 }}><Input value={ref.kind || ''} onChange={(e) => onChange({ ...filter, extensionRef: { ...ref, kind: e.target.value } })} disabled={disabled} /></Form.Item>
      <Form.Item label={t('field.name')} style={{ marginBottom: 0 }}><Input value={ref.name || ''} onChange={(e) => onChange({ ...filter, extensionRef: { ...ref, name: e.target.value } })} disabled={disabled} /></Form.Item>
    </Space>
  }
  if (filter.type === 'RequestRedirect' || filter.type === 'URLRewrite') {
    const key = filter.type === 'RequestRedirect' ? 'requestRedirect' : 'urlRewrite'
    const config = (filter as any)[key] || {}
    const path = config.path || {}
    return <Space direction="vertical" style={{ width: '100%' }}>
      <Space wrap>
        {filter.type === 'RequestRedirect' && <Form.Item label={t('routeFilter.scheme')} style={{ marginBottom: 0 }}><Select allowClear value={config.scheme} options={['http', 'https'].map((value) => ({ value }))} onChange={(scheme) => onChange({ ...filter, [key]: { ...config, scheme } })} disabled={disabled} style={{ width: 120 }} /></Form.Item>}
        <Form.Item label={t('routeFilter.hostname')} style={{ marginBottom: 0 }}><Input value={config.hostname || ''} onChange={(e) => onChange({ ...filter, [key]: { ...config, hostname: e.target.value } })} disabled={disabled} /></Form.Item>
        {filter.type === 'RequestRedirect' && <>
          <Form.Item label={t('field.portOpt')} style={{ marginBottom: 0 }}><InputNumber value={config.port} min={1} max={65535} onChange={(port) => onChange({ ...filter, [key]: { ...config, port: port ?? undefined } })} disabled={disabled} /></Form.Item>
          <Form.Item label={t('routeFilter.statusCode')} style={{ marginBottom: 0 }}><Select value={config.statusCode} allowClear options={[301,302,303,307,308].map((value) => ({ value }))} onChange={(statusCode) => onChange({ ...filter, [key]: { ...config, statusCode } })} disabled={disabled} style={{ width: 100 }} /></Form.Item>
        </>}
      </Space>
      <Space wrap>
        <Form.Item label={t('routeFilter.pathType')} style={{ marginBottom: 0 }}><Select allowClear value={path.type} options={['ReplaceFullPath','ReplacePrefixMatch'].map((value) => ({ value }))} onChange={(type) => {
          const nextPath: Record<string, unknown> = { ...path, type }
          delete nextPath.replaceFullPath
          delete nextPath.replacePrefixMatch
          onChange({ ...filter, [key]: { ...config, path: nextPath } })
        }} disabled={disabled} style={{ width: 210 }} /></Form.Item>
        <Form.Item label={t('routeFilter.replacement')} style={{ marginBottom: 0 }}><Input value={path.type === 'ReplacePrefixMatch' ? path.replacePrefixMatch || '' : path.replaceFullPath || ''} onChange={(e) => {
          const field = path.type === 'ReplacePrefixMatch' ? 'replacePrefixMatch' : 'replaceFullPath'
          onChange({ ...filter, [key]: { ...config, path: { ...path, [field]: e.target.value } } })
        }} disabled={disabled} /></Form.Item>
      </Space>
    </Space>
  }
  if (filter.type === 'RequestMirror') {
    const mirror = (filter as any).requestMirror || { backendRef: {} }
    return <Space direction="vertical" style={{ width: '100%' }}>
      <ObjectRefFields value={mirror.backendRef} onChange={(backendRef) => onChange({ ...filter, requestMirror: { ...mirror, backendRef } })} disabled={disabled} />
      <Space wrap>
        <Form.Item label={t('routeFilter.numerator')} style={{ marginBottom: 0 }}><InputNumber min={0} value={mirror.fraction?.numerator} onChange={(numerator) => onChange({ ...filter, requestMirror: { ...mirror, fraction: { ...mirror.fraction, numerator: numerator ?? 0 } } })} disabled={disabled} /></Form.Item>
        <Form.Item label={t('routeFilter.denominator')} style={{ marginBottom: 0 }}><InputNumber min={1} value={mirror.fraction?.denominator} onChange={(denominator) => onChange({ ...filter, requestMirror: { ...mirror, fraction: { ...mirror.fraction, denominator: denominator ?? undefined } } })} disabled={disabled} /></Form.Item>
        <Form.Item label={t('routeFilter.percentage')} style={{ marginBottom: 0 }}><InputNumber min={0} max={100} value={mirror.percentage} onChange={(percentage) => onChange({ ...filter, requestMirror: { ...mirror, percentage: percentage ?? undefined } })} disabled={disabled} /></Form.Item>
        {(['connectTimeoutMs','writeTimeoutMs','maxBufferedChunks','maxConcurrent','channelFullTimeoutMs'] as const).map((field) => <Form.Item key={field} label={t(`routeFilter.${field}` as any)} style={{ marginBottom: 0 }}><InputNumber min={0} value={mirror[field]} onChange={(v) => onChange({ ...filter, requestMirror: { ...mirror, [field]: v ?? undefined } })} disabled={disabled} /></Form.Item>)}
        <Checkbox checked={mirror.mirrorLog ?? false} onChange={(e) => onChange({ ...filter, requestMirror: { ...mirror, mirrorLog: e.target.checked } })} disabled={disabled}>{t('routeFilter.mirrorLog')}</Checkbox>
      </Space>
    </Space>
  }
  if (filter.type === 'CORS') {
    const cors = (filter as any).cors || {}
    const tagField = (field: string) => <Form.Item key={field} label={t(`routeFilter.${field}` as any)} style={{ marginBottom: 0 }}><Select mode="tags" tokenSeparators={[',']} value={cors[field] || []} onChange={(v) => onChange({ ...filter, cors: { ...cors, [field]: v } })} disabled={disabled} style={{ minWidth: 240 }} /></Form.Item>
    return <Space direction="vertical" style={{ width: '100%' }}>
      {['allowOrigins','allowMethods','allowHeaders','exposeHeaders'].map(tagField)}
      <Space wrap><Form.Item label={t('routeFilter.maxAge')} style={{ marginBottom: 0 }}><InputNumber min={0} value={cors.maxAge} onChange={(maxAge) => onChange({ ...filter, cors: { ...cors, maxAge: maxAge ?? 0 } })} disabled={disabled} /></Form.Item>
      <Checkbox checked={cors.allowCredentials ?? false} onChange={(e) => onChange({ ...filter, cors: { ...cors, allowCredentials: e.target.checked } })} disabled={disabled}>{t('routeFilter.allowCredentials')}</Checkbox></Space>
    </Space>
  }
  if (filter.type === 'ExternalAuth') {
    const auth = ((filter as any).externalAuth || {}) as HTTPExternalAuthFilter
    return <ExternalAuthFilterEditor value={auth} onChange={(externalAuth) => onChange({ ...filter, externalAuth })} disabled={disabled} />
  }
  return null
}

const createFilter = (type: string): RouteFilter => {
  if (type === 'RequestHeaderModifier') return { type: 'RequestHeaderModifier', requestHeaderModifier: {} }
  if (type === 'ResponseHeaderModifier') return { type: 'ResponseHeaderModifier', responseHeaderModifier: {} }
  if (type === 'RequestRedirect') return { type: 'RequestRedirect', requestRedirect: {} }
  if (type === 'URLRewrite') return { type: 'URLRewrite', urlRewrite: {} }
  if (type === 'RequestMirror') return { type: 'RequestMirror', requestMirror: { backendRef: { name: '' } } }
  if (type === 'CORS') return { type: 'CORS', cors: {} }
  if (type === 'ExternalAuth') return { type: 'ExternalAuth', externalAuth: { target: { name: '' }, decision: {} } }
  return { type: 'ExtensionRef', extensionRef: { group: 'edgion.io', kind: 'EdgionPlugins', name: '' } }
}

const FILTER_PAYLOAD_KEYS = [
  'requestHeaderModifier', 'responseHeaderModifier', 'requestRedirect', 'urlRewrite',
  'requestMirror', 'cors', 'externalAuth', 'extensionRef',
] as const

export function switchRouteFilterType(current: RouteFilter, type: string): RouteFilter {
  const next = { ...current } as Record<string, unknown>
  for (const key of FILTER_PAYLOAD_KEYS) delete next[key]
  return { ...next, ...createFilter(type) } as RouteFilter
}

const RouteFiltersEditor: React.FC<Props> = ({ value = [], onChange, disabled = false, protocol = 'http' }) => {
  const t = useT()
  const types = protocol === 'http' ? HTTP_TYPES : GRPC_TYPES
  const update = (index: number, filter: RouteFilter) => {
    const next = [...value]
    next[index] = filter
    onChange?.(next)
  }
  return <Space direction="vertical" style={{ width: '100%' }}>
    {value.map((filter, index) => <Card key={index} size="small" title={`${filter.type} ${index + 1}`}
      extra={!disabled && <Button type="text" danger icon={<MinusCircleOutlined />} onClick={() => onChange?.(value.filter((_, i) => i !== index))}>{t('btn.delete')}</Button>}>
      <Form.Item label={t('routeFilter.type')} style={{ marginBottom: 12 }}>
        <Select value={filter.type} options={types.map((type) => ({ value: type }))} onChange={(type) => update(index, switchRouteFilterType(filter, type))} disabled={disabled} />
      </Form.Item>
      <FilterBody filter={filter} onChange={(next) => update(index, next)} disabled={disabled} />
    </Card>)}
    {!disabled && <Button type="dashed" block icon={<PlusOutlined />} onClick={() => onChange?.([...value, createFilter(types[0])])}>{t('routeFilter.add')}</Button>}
  </Space>
}

export default RouteFiltersEditor
