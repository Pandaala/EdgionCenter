import { Button, Input, Space } from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import { useT } from '@/i18n'

interface ConfigDataRef {
  name: string
  namespace?: string
  [key: string]: unknown
}

/** Edit reference identity only; list data stays in EdgionConfigData. */
export default function ConfigDataRefsEditor({ value, onChange, readOnly, addButtonProps, removeButtonProps }: {
  value: ConfigDataRef[]
  onChange: (value: ConfigDataRef[]) => void
  readOnly: boolean
  addButtonProps?: { 'data-testid': string }
  removeButtonProps?: { 'data-testid': string }
}) {
  const t = useT()
  return <Space direction="vertical" style={{ width: '100%' }}>
    {value.map((ref, index) => <Space key={index} wrap>
      <Input aria-label={t('field.name')} placeholder="name" value={ref.name} disabled={readOnly}
        onChange={(event) => onChange(value.map((entry, current) => current === index ? { ...entry, name: event.target.value } : entry))} />
      <Input aria-label={t('field.namespaceOpt')} placeholder="namespace" value={ref.namespace ?? ''} disabled={readOnly}
        onChange={(event) => onChange(value.map((entry, current) => current === index ? { ...entry, namespace: event.target.value || undefined } : entry))} />
      {!readOnly && <Button {...removeButtonProps} danger icon={<DeleteOutlined />}
        onClick={() => onChange(value.filter((_, current) => current !== index))}>{t('btn.remove')}</Button>}
    </Space>)}
    {!readOnly && <Button {...addButtonProps} type="dashed" icon={<PlusOutlined />}
      onClick={() => onChange([...value, { name: '' }])}>{t('btn.addReference')}</Button>}
  </Space>
}
