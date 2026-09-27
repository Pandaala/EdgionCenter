import { Alert, Button, Card, Space } from 'antd'
import type { WafConfig } from '@/types/edgion-plugins'
import { useT } from '@/i18n'
import StructuredConfigEditor from '../StructuredConfigEditor'
import type { PluginField } from '../pluginCatalog'

const FIELDS: readonly PluginField[] = [
  { name: 'policyRef', kind: 'object', defaultValue: { name: '', namespace: '' } },
  { name: 'activeProfile', kind: 'string' },
  { name: 'activeProfileRef', kind: 'object', defaultValue: { name: '', namespace: '' } },
  { name: 'mode', kind: 'string', options: ['on', 'detectionOnly'] },
  { name: 'priority', kind: 'number' },
]
const BODY_FIELDS: readonly PluginField[] = [
  { name: 'inspection', kind: 'string', options: ['prefix', 'full'] },
  { name: 'prefixSize', kind: 'string' },
]

export default function WafSection({ value, onChange, readOnly }: {
  value: WafConfig | undefined
  onChange: (value: WafConfig | undefined) => void
  readOnly: boolean
}) {
  const t = useT()
  const { requestBody, ...config } = value ?? {}
  return (
    <Card title="WAF" size="small" style={{ marginTop: 16 }}>
      <Space direction="vertical" style={{ width: '100%' }}>
        <Alert type="info" message={t('plugins.wafHelp')} />
        {value != null && <>
          <StructuredConfigEditor fields={FIELDS} value={config} readOnly={readOnly}
            onChange={(next) => onChange({ ...next, ...(requestBody === undefined ? {} : { requestBody }) } as WafConfig)} />
          <Card title="requestBody" size="small">
            <StructuredConfigEditor fields={BODY_FIELDS} value={requestBody ?? {}} readOnly={readOnly}
              onChange={(next) => onChange({ ...value, requestBody: next })} />
          </Card>
        </>}
        {!readOnly && <Button danger={value != null} onClick={() => onChange(value == null ? { policyRef: { name: '' } } : undefined)}>
          {value == null ? t('plugins.addWaf') : t('plugins.removeWaf')}
        </Button>}
      </Space>
    </Card>
  )
}
