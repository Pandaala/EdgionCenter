import { Button, Card, Collapse, Form, Select, Space, Switch } from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import MetadataSection from '../common/MetadataSection'
import StructuredConfigEditor from '../EdgionPlugins/StructuredConfigEditor'
import type { PluginField } from '../EdgionPlugins/pluginCatalog'
import type { EdgionStreamPlugins, StreamPlugin } from '@/types/edgion-stream-plugins'
import { useT } from '@/i18n'

interface Props {
  data: EdgionStreamPlugins
  onChange: (data: EdgionStreamPlugins) => void
  readOnly?: boolean
  isCreate?: boolean
}

const STAGE_ONE_TYPES = ['IpRestriction', 'GlobalConnectionIpRestriction', 'ConnectionRateLimit'] as const
const TLS_ROUTE_TYPES = ['IpRestriction'] as const

const FIELDS: Record<string, readonly PluginField[]> = {
  IpRestriction: [
    { name: 'allow', kind: 'array' }, { name: 'deny', kind: 'array' },
    { name: 'defaultAction', kind: 'string', options: ['allow', 'deny'] },
    { name: 'allowRefs', kind: 'array' }, { name: 'denyRefs', kind: 'array' },
  ],
  TlsRouteIpRestriction: [
    { name: 'allow', kind: 'array' }, { name: 'deny', kind: 'array' },
    { name: 'ipSource', kind: 'string', options: ['clientIp', 'remoteAddr'] },
    { name: 'message', kind: 'string' }, { name: 'status', kind: 'number' },
    { name: 'defaultAction', kind: 'string', options: ['allow', 'deny'] },
    { name: 'allowRefs', kind: 'array' }, { name: 'denyRefs', kind: 'array' },
  ],
  GlobalConnectionIpRestriction: [
    { name: 'enable', kind: 'boolean' }, { name: 'activeProfile', kind: 'string' },
    { name: 'profiles', kind: 'object' }, { name: 'description', kind: 'string' },
    { name: 'activeProfileRef', kind: 'object' },
  ],
  ConnectionRateLimit: [
    { name: 'redisRef', kind: 'string' },
    { name: 'algorithm', kind: 'string', options: ['SlidingWindow', 'FixedWindow', 'TokenBucket'] },
    { name: 'keyPrefix', kind: 'string' }, { name: 'perListener', kind: 'object' },
    { name: 'perSourceIp', kind: 'object' }, { name: 'redisTimeout', kind: 'string' },
  ],
}

function StreamStageEditor({ title, entries, types, tlsRoute, readOnly, onChange }: {
  title: string
  entries: StreamPlugin[]
  types: readonly string[]
  tlsRoute: boolean
  readOnly: boolean
  onChange: (entries: StreamPlugin[]) => void
}) {
  const t = useT()
  return (
    <Space direction="vertical" style={{ width: '100%' }}>
      {entries.map((entry, index) => (
        <Card key={index} size="small" title={`${index + 1}. ${entry.type}`} extra={!readOnly && (
          <Button data-testid="edgionstreamplugins-entry-remove" danger icon={<DeleteOutlined />} onClick={() => onChange(entries.filter((_, current) => current !== index))}>{t('btn.delete')}</Button>
        )}>
          <Space direction="vertical" style={{ width: '100%' }}>
            <Space wrap>
              <Form.Item label={t('field.pluginType')}>
                <Select value={entry.type} disabled={readOnly} options={types.map((type) => ({ value: type, label: type }))} onChange={(type) => {
                  const next = [...entries]; next[index] = { ...entry, type, config: {} }; onChange(next)
                }} style={{ width: 280 }} />
              </Form.Item>
              <Form.Item label={t('field.enabled')}>
                <Switch checked={entry.enable !== false} disabled={readOnly} onChange={(enable) => {
                  const next = [...entries]; next[index] = { ...entry, enable }; onChange(next)
                }} />
              </Form.Item>
            </Space>
            <StructuredConfigEditor
              fields={FIELDS[tlsRoute && entry.type === 'IpRestriction' ? 'TlsRouteIpRestriction' : entry.type] ?? []}
              value={(entry.config ?? {}) as Record<string, unknown>}
              readOnly={readOnly}
              onChange={(config) => {
                const next = [...entries]; next[index] = { ...entry, config }; onChange(next)
              }}
            />
          </Space>
        </Card>
      ))}
      {!readOnly && <Button data-testid="edgionstreamplugins-entry-add" block type="dashed" icon={<PlusOutlined />} onClick={() => onChange([...entries, { enable: true, type: types[0], config: {} }])}>{t('plugins.addToStage')}</Button>}
      {entries.length === 0 && readOnly && <span>{title}: 0</span>}
    </Space>
  )
}

export default function EdgionStreamPluginsForm({ data, onChange, readOnly = false, isCreate = true }: Props) {
  const t = useT()
  const plugins = data.spec.plugins ?? []
  const tlsRoutePlugins = data.spec.tlsRoutePlugins ?? []
  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection value={data.metadata} onChange={(metadata) => onChange({ ...data, metadata })} disabled={readOnly} isCreate={isCreate} />
        <Collapse defaultActiveKey={['connection']} items={[
          {
            key: 'connection', label: `${t('plugins.connectionStage')} (${plugins.length})`,
            children: <StreamStageEditor title="ConnectionFilter" entries={plugins} types={STAGE_ONE_TYPES} tlsRoute={false} readOnly={readOnly} onChange={(next) => onChange({ ...data, spec: { ...data.spec, plugins: next } })} />,
          },
          {
            key: 'tls', label: `${t('plugins.tlsRouteStage')} (${tlsRoutePlugins.length})`,
            children: <StreamStageEditor title="TlsRoute" entries={tlsRoutePlugins} types={TLS_ROUTE_TYPES} tlsRoute readOnly={readOnly} onChange={(next) => onChange({ ...data, spec: { ...data.spec, tlsRoutePlugins: next } })} />,
          },
        ]} />
      </Space>
    </Form>
  )
}
