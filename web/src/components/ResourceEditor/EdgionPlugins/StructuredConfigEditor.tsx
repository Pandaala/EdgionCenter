import { Button, Card, Form, Input, InputNumber, Select, Space, Switch } from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import type { FieldKind, PluginField } from './pluginCatalog'
import { useT } from '@/i18n'

function valueKind(value: unknown): FieldKind | 'null' {
  if (value === null) return 'null'
  if (Array.isArray(value)) return 'array'
  if (typeof value === 'boolean') return 'boolean'
  if (typeof value === 'number') return 'number'
  if (typeof value === 'object') return 'object'
  return 'string'
}

function defaultForKind(kind: FieldKind | 'null'): unknown {
  if (kind === 'array') return []
  if (kind === 'object') return {}
  if (kind === 'boolean') return false
  if (kind === 'number') return 0
  if (kind === 'null') return null
  return ''
}

const DynamicValueEditor = ({ value, onChange, readOnly, depth = 0 }: {
  value: unknown
  onChange: (value: unknown) => void
  readOnly: boolean
  depth?: number
}) => {
  const t = useT()
  const kind = valueKind(value)
  if (kind === 'boolean') return <Switch checked={value as boolean} disabled={readOnly} onChange={onChange} />
  if (kind === 'number') return <InputNumber value={value as number} disabled={readOnly} onChange={(next) => onChange(next ?? 0)} style={{ width: '100%' }} />
  if (kind === 'array') {
    const items = value as unknown[]
    return (
      <Space direction="vertical" style={{ width: '100%' }}>
        {items.map((item, index) => (
          <Card key={index} size="small">
            <Space direction="vertical" style={{ width: '100%' }}>
              <Space>
                <Select
                  aria-label={t('plugins.valueType')}
                  value={valueKind(item)}
                  disabled={readOnly}
                  options={['string', 'number', 'boolean', 'object', 'array', 'null'].map((entry) => ({ value: entry, label: entry }))}
                  onChange={(nextKind) => {
                    const next = [...items]
                    next[index] = defaultForKind(nextKind)
                    onChange(next)
                  }}
                />
                {!readOnly && <Button danger icon={<DeleteOutlined />} onClick={() => onChange(items.filter((_, current) => current !== index))}>{t('btn.remove')}</Button>}
              </Space>
              <DynamicValueEditor
                value={item}
                readOnly={readOnly}
                depth={depth + 1}
                onChange={(nextItem) => {
                  const next = [...items]
                  next[index] = nextItem
                  onChange(next)
                }}
              />
            </Space>
          </Card>
        ))}
        {!readOnly && <Button type="dashed" icon={<PlusOutlined />} onClick={() => onChange([...items, ''])}>{t('btn.addItem')}</Button>}
      </Space>
    )
  }
  if (kind === 'object') {
    const object = value as Record<string, unknown>
    const entries = Object.entries(object)
    return (
      <Space direction="vertical" style={{ width: '100%' }}>
        {entries.map(([key, item], index) => (
          <Card key={`${key}-${index}`} size="small">
            <Space direction="vertical" style={{ width: '100%' }}>
              <Space style={{ width: '100%' }}>
                <Input
                  aria-label={t('plugins.fieldName')}
                  value={key}
                  disabled={readOnly}
                  onChange={(event) => {
                    const nextKey = event.target.value
                    const next = { ...object }
                    delete next[key]
                    next[nextKey] = item
                    onChange(next)
                  }}
                />
                <Select
                  aria-label={t('plugins.valueType')}
                  value={valueKind(item)}
                  disabled={readOnly}
                  options={['string', 'number', 'boolean', 'object', 'array', 'null'].map((entry) => ({ value: entry, label: entry }))}
                  onChange={(nextKind) => onChange({ ...object, [key]: defaultForKind(nextKind) })}
                />
                {!readOnly && <Button danger icon={<DeleteOutlined />} onClick={() => {
                  const next = { ...object }; delete next[key]; onChange(next)
                }}>{t('btn.remove')}</Button>}
              </Space>
              <DynamicValueEditor value={item} readOnly={readOnly} depth={depth + 1} onChange={(nextItem) => onChange({ ...object, [key]: nextItem })} />
            </Space>
          </Card>
        ))}
        {!readOnly && depth < 8 && <Button type="dashed" icon={<PlusOutlined />} onClick={() => {
          let index = entries.length + 1
          while (`field${index}` in object) index += 1
          onChange({ ...object, [`field${index}`]: '' })
        }}>{t('btn.addField')}</Button>}
      </Space>
    )
  }
  if (kind === 'null') return <span>{t('plugins.nullValue')}</span>
  return <Input value={String(value ?? '')} disabled={readOnly} onChange={(event) => onChange(event.target.value)} />
}

const KnownFieldEditor = ({ field, value, onChange, readOnly }: {
  field: PluginField
  value: unknown
  onChange: (value: unknown) => void
  readOnly: boolean
}) => {
  if (field.options) {
    return <Select allowClear value={value as string | undefined} disabled={readOnly} options={field.options.map((entry) => ({ value: entry, label: entry }))} onChange={onChange} style={{ width: '100%' }} />
  }
  if (field.kind === 'boolean') return <Switch checked={value === true} disabled={readOnly} onChange={onChange} />
  if (field.kind === 'number') return <InputNumber value={value as number | undefined} disabled={readOnly} onChange={(next) => onChange(next)} style={{ width: '100%' }} />
  if (field.kind === 'code') return <Input.TextArea value={value as string | undefined} disabled={readOnly} onChange={(event) => onChange(event.target.value)} autoSize={{ minRows: 5, maxRows: 16 }} style={{ fontFamily: 'monospace' }} />
  if (field.kind === 'string') return <Input value={value as string | undefined} disabled={readOnly} onChange={(event) => onChange(event.target.value)} />
  return <DynamicValueEditor value={value ?? defaultForKind(field.kind)} onChange={onChange} readOnly={readOnly} />
}

export default function StructuredConfigEditor({ fields, value, onChange, readOnly }: {
  fields: readonly PluginField[]
  value: Record<string, unknown>
  onChange: (value: Record<string, unknown>) => void
  readOnly: boolean
}) {
  const t = useT()
  const known = new Set(fields.map((field) => field.name))
  const unknown = Object.fromEntries(Object.entries(value).filter(([key]) => !known.has(key)))
  return (
    <Space direction="vertical" style={{ width: '100%' }}>
      {fields.map((field) => (
        <Form.Item key={field.name} label={field.name} style={{ marginBottom: 8 }}>
          <Space.Compact style={{ width: '100%' }}>
            <KnownFieldEditor field={field} value={value[field.name]} readOnly={readOnly} onChange={(next) => onChange({ ...value, [field.name]: next })} />
            {!readOnly && value[field.name] !== undefined && <Button danger onClick={() => {
              const next = { ...value }; delete next[field.name]; onChange(next)
            }}>{t('btn.clear')}</Button>}
          </Space.Compact>
        </Form.Item>
      ))}
      {(Object.keys(unknown).length > 0 || !readOnly) && (
        <Card title={t('plugins.additionalFields')} size="small">
          <DynamicValueEditor
            value={unknown}
            readOnly={readOnly}
            onChange={(nextUnknown) => onChange({
              ...Object.fromEntries(Object.entries(value).filter(([key]) => known.has(key))),
              ...(nextUnknown as Record<string, unknown>),
            })}
          />
        </Card>
      )}
    </Space>
  )
}
