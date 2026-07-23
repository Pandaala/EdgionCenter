import { Button, Card, Collapse, Form, Input, Select, Space, Switch, Tag } from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import type { EdgionPluginsSpec, PluginEntry } from '@/types/edgion-plugins'
import { useT } from '@/i18n'
import StructuredConfigEditor from '../StructuredConfigEditor'
import {
  HTTP_PLUGIN_CATALOG,
  PLUGIN_DEFINITION_BY_TYPE,
  defaultConfigForPlugin,
  pluginTypesForStage,
  type PluginStage,
} from '../pluginCatalog'
import { pluginAcceptsBodyRequirement } from '@/utils/edgionplugins'

const STAGES: Array<{ key: PluginStage; label: string }> = [
  { key: 'requestPlugins', label: 'RequestFilter' },
  { key: 'upstreamResponseFilterPlugins', label: 'UpstreamResponseFilter' },
  { key: 'upstreamResponseBodyFilterPlugins', label: 'UpstreamResponseBodyFilter' },
  { key: 'upstreamResponsePlugins', label: 'UpstreamResponse' },
]

export default function PluginStagesSection({ value, onChange, readOnly }: {
  value: EdgionPluginsSpec
  onChange: (value: EdgionPluginsSpec) => void
  readOnly: boolean
}) {
  const t = useT()
  const updateEntries = (stage: PluginStage, entries: PluginEntry[]) => onChange({ ...value, [stage]: entries })
  const panels = STAGES.map(({ key: stage, label }) => {
    const entries = value[stage] ?? []
    const availableTypes = pluginTypesForStage(stage)
    return {
      key: stage,
      label: <Space><span>{label}</span><Tag>{entries.length}</Tag></Space>,
      children: (
        <Space direction="vertical" style={{ width: '100%' }}>
          {entries.map((entry, index) => {
            const definition = PLUGIN_DEFINITION_BY_TYPE.get(entry.type)
            return (
              <Card
                key={index}
                size="small"
                title={`${index + 1}. ${entry.type}`}
                extra={!readOnly && <Button data-testid="edgionplugins-entry-remove" danger icon={<DeleteOutlined />} onClick={() => updateEntries(stage, entries.filter((_, current) => current !== index))}>{t('btn.delete')}</Button>}
              >
                <Space direction="vertical" style={{ width: '100%' }}>
                  <Space wrap align="start">
                    <Form.Item label={t('field.pluginType')} required>
                      <Select
                        value={entry.type}
                        disabled={readOnly}
                        showSearch
                        style={{ width: 260 }}
                        options={availableTypes.map((type) => ({ value: type, label: type }))}
                        onChange={(type) => {
                          const next = [...entries]
                          const nextEntry = { ...entry, type, config: defaultConfigForPlugin(type) } as PluginEntry
                          if (!pluginAcceptsBodyRequirement(stage, type, nextEntry.config)) {
                            delete (nextEntry as PluginEntry & { body?: unknown }).body
                          }
                          next[index] = nextEntry
                          updateEntries(stage, next)
                        }}
                      />
                    </Form.Item>
                    <Form.Item label={t('field.enabled')}>
                      <Switch checked={entry.enable !== false} disabled={readOnly} onChange={(enable) => {
                        const next = [...entries]; next[index] = { ...entry, enable }; updateEntries(stage, next)
                      }} />
                    </Form.Item>
                    <Form.Item label={t('plugins.alias')} tooltip={t('plugins.aliasHelp')}>
                      <Input value={entry.alias} disabled={readOnly} maxLength={32} onChange={(event) => {
                        const next = [...entries]; next[index] = { ...entry, alias: event.target.value || undefined }; updateEntries(stage, next)
                      }} />
                    </Form.Item>
                  </Space>
                  <Card title={t('plugins.conditions')} size="small">
                    <StructuredConfigEditor
                      fields={[{ name: 'skip', kind: 'array', defaultValue: [] }, { name: 'run', kind: 'array', defaultValue: [] }]}
                      value={(entry.conditions ?? {}) as Record<string, unknown>}
                      readOnly={readOnly}
                      onChange={(conditions) => {
                        const next = [...entries]
                        next[index] = { ...entry, conditions }
                        updateEntries(stage, next)
                      }}
                    />
                  </Card>
                  {pluginAcceptsBodyRequirement(stage, entry.type, entry.config) && (
                    <Card title={t('plugins.body')} size="small">
                      <StructuredConfigEditor
                        fields={[
                          { name: 'maxBodySize', kind: 'string' },
                          { name: 'conditions', kind: 'object', defaultValue: {} },
                          { name: 'onReadFailure', kind: 'string', options: ['failClose', 'failOpen'] },
                        ]}
                        value={(('body' in entry && entry.body) || {}) as Record<string, unknown>}
                        readOnly={readOnly}
                        onChange={(body) => {
                          const next = [...entries]
                          next[index] = { ...entry, body } as PluginEntry
                          updateEntries(stage, next)
                        }}
                      />
                    </Card>
                  )}
                  <Card title={t('plugins.dye')} size="small">
                    <StructuredConfigEditor
                      fields={[
                        { name: 'request', kind: 'array', defaultValue: [] },
                        { name: 'response', kind: 'array', defaultValue: [] },
                      ]}
                      value={(entry.dye ?? {}) as Record<string, unknown>}
                      readOnly={readOnly}
                      onChange={(dye) => {
                        const next = [...entries]
                        next[index] = { ...entry, dye }
                        updateEntries(stage, next)
                      }}
                    />
                  </Card>
                  <Card title={t('plugins.config')} size="small">
                    {definition ? (
                      <StructuredConfigEditor
                        fields={definition.fields}
                        value={entry.config ?? {}}
                        readOnly={readOnly}
                        onChange={(config) => {
                          const next = [...entries]
                          const nextEntry = { ...entry, config } as PluginEntry
                          if (!pluginAcceptsBodyRequirement(stage, entry.type, config)) {
                            delete (nextEntry as PluginEntry & { body?: unknown }).body
                          }
                          next[index] = nextEntry
                          updateEntries(stage, next)
                        }}
                      />
                    ) : <Tag color="red">{t('plugins.unknownType')}</Tag>}
                  </Card>
                </Space>
              </Card>
            )
          })}
          {!readOnly && availableTypes.length > 0 && (
            <Button data-testid="edgionplugins-entry-add" type="dashed" block icon={<PlusOutlined />} onClick={() => {
              const type = availableTypes[0]
              updateEntries(stage, [...entries, { enable: true, type, config: defaultConfigForPlugin(type) }])
            }}>{t('plugins.addToStage')}</Button>
          )}
        </Space>
      ),
    }
  })
  return (
    <Card title={t('plugins.configuration')} size="small" extra={<Tag color="blue">{HTTP_PLUGIN_CATALOG.length}</Tag>}>
      <Collapse items={panels} defaultActiveKey={['requestPlugins']} />
    </Card>
  )
}
