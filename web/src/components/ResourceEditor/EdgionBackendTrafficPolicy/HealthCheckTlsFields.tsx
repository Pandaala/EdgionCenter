import { useEffect, useState } from 'react'
import { Alert, Form, Input } from 'antd'
import type { HealthCheckTlsConfig } from '@/types/edgion-backend-traffic-policy'

export default function HealthCheckTlsFields({
  value, onChange, readOnly, onDraftValidationChange,
}: {
  value?: HealthCheckTlsConfig
  onChange: (value: HealthCheckTlsConfig) => void
  readOnly: boolean
  onDraftValidationChange?: (errors: string[]) => void
}) {
  const [draft, setDraft] = useState(() => JSON.stringify(value ?? {}, null, 2))
  const [error, setError] = useState<string>()
  useEffect(() => {
    setDraft(JSON.stringify(value ?? {}, null, 2))
    setError(undefined)
    onDraftValidationChange?.([])
  }, [value, onDraftValidationChange])
  return (
    <Form.Item label="Probe TLS" help="Encrypted probes require verification and validation.hostname. Configure CA and client-certificate references in the policy namespace. Controller validates certificate identity and trust.">
      <Input.TextArea
        aria-label="Probe TLS"
        disabled={readOnly}
        rows={8}
        value={draft}
        onChange={(event) => {
          setDraft(event.target.value)
          try {
            const parsed: unknown = JSON.parse(event.target.value)
            if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('Probe TLS must be a JSON object')
            setError(undefined)
            onDraftValidationChange?.([])
            onChange(parsed as HealthCheckTlsConfig)
          } catch {
            const message = 'Probe TLS must be a valid JSON object'
            setError(message)
            onDraftValidationChange?.([message])
          }
        }}
      />
      {error && <Alert type="error" message={error} />}
    </Form.Item>
  )
}
