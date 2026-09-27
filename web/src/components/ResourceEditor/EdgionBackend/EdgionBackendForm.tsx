import { Button, Card, Form, Input, InputNumber, Select, Space, Switch } from 'antd'
import type { AiBackendSpec, AiCredential, AiModel, EdgionBackend } from '@/types/edgion-backend'
import MetadataSection from '../common/MetadataSection'
import { useT } from '@/i18n'

interface Props {
  data: EdgionBackend
  onChange: (value: EdgionBackend) => void
  readOnly?: boolean
  isCreate?: boolean
}

export default function EdgionBackendForm({ data, onChange, readOnly, isCreate }: Props) {
  const t = useT()
  const ai = data.spec.ai
  const patch = (value: Partial<AiBackendSpec>) => onChange({ ...data, spec: { ...data.spec, ai: { ...ai, ...value } } })
  const patchPool = (value: Partial<AiBackendSpec['credentialPool']>) => patch({ credentialPool: { ...ai.credentialPool, ...value } })
  const patchCredential = (index: number, value: Partial<AiCredential>) => patchPool({
    credentials: ai.credentialPool.credentials.map((item, i) => i === index ? { ...item, ...value } : item),
  })
  const patchModel = (index: number, value: Partial<AiModel>) => patch({
    models: ai.models.map((item, i) => i === index ? { ...item, ...value } : item),
  })
  return (
    <Form layout="vertical" disabled={readOnly}>
      <Space direction="vertical" style={{ width: '100%' }}>
        <MetadataSection value={data.metadata} onChange={(metadata) => onChange({ ...data, metadata })} disabled={readOnly} isCreate={isCreate} />
        <Card title={t('ai.provider')} size="small">
          <Form.Item label={t('ai.provider')} required>
            <Select value={ai.provider} options={['OpenAI', 'OpenAICompatible', 'Anthropic'].map((value) => ({ value, label: value }))} onChange={(provider) => patch({ provider })} />
          </Form.Item>
          <Form.Item label={t('ai.endpoint')} required={ai.provider === 'OpenAICompatible'}>
            <Input value={ai.endpoint} onChange={(event) => patch({ endpoint: event.target.value || undefined })} />
          </Form.Item>
          <Form.Item label={t('ai.insecureHttp')}><Switch checked={ai.allowInsecureHttp ?? false} onChange={(allowInsecureHttp) => patch({ allowInsecureHttp })} /></Form.Item>
        </Card>
        <Card title={t('ai.credentials')} size="small">
          {ai.credentialPool.credentials.map((credential, index) => (
            <Card key={index} size="small" title={`${t('ai.credential')} ${index + 1}`} style={{ marginBottom: 12 }}>
              <Form.Item label={t('field.name')} required><Input value={credential.name} onChange={(event) => patchCredential(index, { name: event.target.value })} /></Form.Item>
              <Form.Item label={t('ai.secretName')} required><Input value={credential.secretRef.name} onChange={(event) => patchCredential(index, { secretRef: { ...credential.secretRef, name: event.target.value } })} /></Form.Item>
              <Form.Item label={t('ai.secretNamespace')}><Input value={credential.secretRef.namespace} onChange={(event) => patchCredential(index, { secretRef: { ...credential.secretRef, namespace: event.target.value || undefined } })} /></Form.Item>
              <Form.Item label={t('ai.weight')}><InputNumber min={1} max={1000000} precision={0} value={credential.weight} placeholder="1" onChange={(weight) => patchCredential(index, { weight: weight ?? undefined })} /></Form.Item>
              {(['requestsPerMinute', 'tokensPerMinute'] as const).map((key) => (
                <Form.Item key={key} label={t(`ai.${key}`)}><InputNumber min={1} max={Number.MAX_SAFE_INTEGER} precision={0} value={credential.limits?.[key]} onChange={(value) => patchCredential(index, { limits: { ...credential.limits, [key]: value ?? undefined } })} /></Form.Item>
              ))}
              <Button danger onClick={() => patchPool({ credentials: ai.credentialPool.credentials.filter((_, i) => i !== index) })}>{t('btn.delete')}</Button>
            </Card>
          ))}
          <Button onClick={() => patchPool({ credentials: [...ai.credentialPool.credentials, { name: '', secretRef: { name: '' } }] })}>{t('ai.addCredential')}</Button>
          <Form.Item label={t('ai.redisRef')}><Input value={ai.credentialPool.redisRef} placeholder="namespace/name" onChange={(event) => patchPool({ redisRef: event.target.value || undefined })} /></Form.Item>
          <Form.Item label={t('ai.redisFailure')}><Select allowClear placeholder="Deny" value={ai.credentialPool.onRedisFailure} options={['Deny', 'Allow'].map((value) => ({ value, label: value }))} onChange={(onRedisFailure) => patchPool({ onRedisFailure })} /></Form.Item>
        </Card>
        <Card title={t('ai.models')} size="small">
          {ai.models.map((model, index) => (
            <Card key={index} size="small" style={{ marginBottom: 12 }}>
              <Form.Item label={t('ai.modelName')} required><Input value={model.name} onChange={(event) => patchModel(index, { name: event.target.value })} /></Form.Item>
              <Form.Item label={t('ai.aliases')}><Select mode="tags" value={model.aliases} onChange={(aliases) => patchModel(index, { aliases })} /></Form.Item>
              <Form.Item label={t('ai.public')}><Switch checked={model.public ?? true} onChange={(visible) => patchModel(index, { public: visible })} /></Form.Item>
              <Form.Item label={t('ai.pricing')}><Switch checked={model.pricing !== undefined} onChange={(enabled) => patchModel(index, { pricing: enabled ? { inputPerMillionUsd: '0', outputPerMillionUsd: '0' } : undefined })} /></Form.Item>
              {model.pricing && (['inputPerMillionUsd', 'outputPerMillionUsd', 'cacheReadInputPerMillionUsd', 'cacheWriteInputPerMillionUsd'] as const).map((key) => (
                <Form.Item key={key} label={t(`ai.${key}`)}><Input value={model.pricing?.[key]} onChange={(event) => patchModel(index, { pricing: { ...model.pricing!, [key]: event.target.value } })} /></Form.Item>
              ))}
              <Button danger onClick={() => patch({ models: ai.models.filter((_, i) => i !== index) })}>{t('btn.delete')}</Button>
            </Card>
          ))}
          <Button onClick={() => patch({ models: [...ai.models, { name: '' }] })}>{t('ai.addModel')}</Button>
        </Card>
        <Card title={t('ai.defaults')} size="small">
          <Form.Item label={t('ai.streaming')}><Switch checked={ai.defaults?.streaming ?? true} onChange={(streaming) => patch({ defaults: { ...ai.defaults, streaming } })} /></Form.Item>
          <Form.Item label={t('ai.maxOutputTokens')}><InputNumber min={1} max={4294967295} precision={0} value={ai.defaults?.maxOutputTokens} onChange={(value) => patch({ defaults: { ...ai.defaults, maxOutputTokens: value ?? undefined } })} /></Form.Item>
        </Card>
      </Space>
    </Form>
  )
}
