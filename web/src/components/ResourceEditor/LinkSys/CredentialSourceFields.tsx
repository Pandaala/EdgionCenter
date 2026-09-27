import { Card, Form, Input, InputNumber, Select, Switch } from 'antd'
import type { CredentialSourceConfig, SecretObjectReference } from '@/types/link-sys'
import JsonValueField from '../common/JsonValueField'

export default function CredentialSourceFields({ config, onChange, readOnly }: {
  config: CredentialSourceConfig
  onChange: (config: CredentialSourceConfig) => void
  readOnly: boolean
}) {
  const provider = config.provider
  const auth = provider.clientAuthentication
  const updateProvider = (patch: Partial<typeof provider>) => onChange({ ...config, provider: { ...provider, ...patch } })
  const updateAuth = (patch: Partial<typeof auth>) => updateProvider({ clientAuthentication: { ...auth, ...patch } })
  const reference = (field: 'activeSecretRef' | 'previousSecretRef', ref: SecretObjectReference | undefined) => (
    <>
      <Form.Item label={field === 'activeSecretRef' ? 'Active Secret name' : 'Previous Secret name'} required>
        <Input value={ref?.name ?? ''} disabled={readOnly} onChange={(event) => updateAuth({ [field]: { ...ref, name: event.target.value } })} />
      </Form.Item>
      <Form.Item label="Secret namespace"><Input value={ref?.namespace ?? ''} disabled={readOnly} onChange={(event) => updateAuth({ [field]: { ...ref, name: ref?.name ?? '', namespace: event.target.value || undefined } })} /></Form.Item>
    </>
  )
  return (
    <Card title="OAuth2 credential source" size="small">
      <Form.Item label="Token endpoint" required><Input data-testid="linksys-credential-endpoint" value={provider.tokenEndpoint} disabled={readOnly} placeholder="https://issuer.example.com/token" onChange={(event) => updateProvider({ tokenEndpoint: event.target.value })} /></Form.Item>
      <Form.Item label="Client authentication"><Input value="clientSecretBasic" readOnly /></Form.Item>
      {reference('activeSecretRef', auth.activeSecretRef)}
      <Form.Item label="Previous bootstrap Secret"><Switch checked={auth.previousSecretRef != null} disabled={readOnly} onChange={(enabled) => updateAuth({ previousSecretRef: enabled ? { name: '' } : undefined })} /></Form.Item>
      {auth.previousSecretRef && reference('previousSecretRef', auth.previousSecretRef)}
      <Form.Item label="Scopes"><Select mode="tags" value={provider.scopes ?? []} disabled={readOnly} onChange={(scopes) => updateProvider({ scopes })} style={{ width: '100%' }} /></Form.Item>
      <Form.Item label="Custom TLS policy"><Switch checked={provider.tls?.enabled ?? false} disabled={readOnly} onChange={(enabled) => updateProvider({ tls: { ...provider.tls, enabled } })} /></Form.Item>
      <Form.Item label="TLS configuration"><JsonValueField value={provider.tls ?? {}} readOnly={readOnly} onChange={(tls) => updateProvider({ tls })} /></Form.Item>
      {([
        ['interval', 'Rotation interval', '15m'],
        ['requestTimeout', 'Request timeout', '10s'],
        ['retryInitialBackoff', 'Initial retry backoff', '2s'],
        ['retryMaxBackoff', 'Maximum retry backoff', '2m'],
      ] as const).map(([field, label, placeholder]) => (
        <Form.Item label={label} key={field}><Input value={config.rotation?.[field] ?? ''} placeholder={placeholder} disabled={readOnly} onChange={(event) => onChange({ ...config, rotation: { ...config.rotation, [field]: event.target.value || undefined } })} /></Form.Item>
      ))}
      <Form.Item label="Block private destinations"><Switch checked={config.egress?.blockPrivate ?? true} disabled={readOnly} onChange={(blockPrivate) => onChange({ ...config, egress: { ...config.egress, blockPrivate } })} /></Form.Item>
      <Form.Item label="Persist published credentials"><Switch checked={config.publication?.persist ?? true} disabled={readOnly} onChange={(persist) => onChange({ ...config, publication: { ...config.publication, persist } })} /></Form.Item>
      <Form.Item label="Maximum in-memory credential keys"><InputNumber value={config.publication?.memoryMaxKeys} placeholder="10000" min={1} max={10000} precision={0} disabled={readOnly} onChange={(memoryMaxKeys) => onChange({ ...config, publication: { ...config.publication, memoryMaxKeys: memoryMaxKeys ?? undefined } })} /></Form.Item>
    </Card>
  )
}
