import React from 'react'
import { Card, Form, Input, InputNumber, Select, Space } from 'antd'
import MetadataSection from '../common/MetadataSection'
import type {
  ElasticsearchConfig,
  EtcdConfig,
  HttpDnsConfig,
  KafkaConfig,
  LinkSys,
  LinkSysConfig,
  LinkSysType,
  RedisConfig,
  SecretAuth,
  WebhookConfig,
} from '@/types/link-sys'
import { createConfig, withWebhookMethod, withWebhookUrl } from '@/utils/linksys'
import { useT } from '@/i18n'

interface LinkSysFormProps {
  data: LinkSys
  onChange: (data: LinkSys) => void
  readOnly?: boolean
  isCreate?: boolean
}

const LinkSysForm: React.FC<LinkSysFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()
  const type = data.spec.type
  const config = data.spec.config

  const updateConfig = (partial: Partial<LinkSysConfig>) =>
    onChange({ ...data, spec: { ...data.spec, config: { ...config, ...partial } as LinkSysConfig } })

  const updateSecretRef = (auth: SecretAuth | undefined, partial: { name?: string; namespace?: string }) => {
    const name = partial.name ?? auth?.secretRef.name ?? ''
    const namespace = partial.namespace ?? auth?.secretRef.namespace
    if (!name) return undefined
    return { secretRef: { name, namespace: namespace || undefined } }
  }

  const handleTypeChange = (newType: LinkSysType) =>
    onChange({ ...data, spec: { type: newType, config: createConfig(newType) } })

  const renderSecretRef = (
    auth: SecretAuth | undefined,
    onAuthChange: (next: SecretAuth | undefined) => void,
  ) => (
    <Space.Compact block>
      <Input
        aria-label={t('field.secretName')}
        value={auth?.secretRef.name || ''}
        onChange={(e) => onAuthChange(updateSecretRef(auth, { name: e.target.value }))}
        disabled={readOnly}
        placeholder={t('field.secretName')}
      />
      <Input
        aria-label={t('field.secretNs')}
        value={auth?.secretRef.namespace || ''}
        onChange={(e) => onAuthChange(updateSecretRef(auth, { namespace: e.target.value }))}
        disabled={readOnly}
        placeholder={t('field.secretNs')}
      />
    </Space.Compact>
  )

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        <Card title={t('section.connType')} size="small">
          <Form.Item label={t('field.connType')} required style={{ marginBottom: 0 }}>
            <Select value={type} onChange={handleTypeChange} disabled={readOnly} style={{ width: 200 }}>
              <Select.Option value="redis">Redis</Select.Option>
              <Select.Option value="elasticsearch">Elasticsearch</Select.Option>
              <Select.Option value="etcd">etcd</Select.Option>
              <Select.Option value="webhook">Webhook</Select.Option>
              <Select.Option value="kafka">Kafka</Select.Option>
              <Select.Option value="httpdns">HTTP DNS</Select.Option>
            </Select>
          </Form.Item>
        </Card>

        {type === 'redis' && (() => {
          const redis = config as RedisConfig
          return (
            <Card title={t('linksys.redis')} size="small">
              <Form.Item label={t('field.addresses')} required style={{ marginBottom: 8 }}>
                <Select mode="tags" value={redis.endpoints || []}
                  onChange={(endpoints) => updateConfig({ endpoints } as Partial<RedisConfig>)}
                  disabled={readOnly} placeholder="redis://127.0.0.1:6379" style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item label={t('field.secretName')} style={{ marginBottom: 8 }}>
                {renderSecretRef(redis.auth, (auth) => updateConfig({ auth } as Partial<RedisConfig>))}
              </Form.Item>
              <Form.Item label={t('field.dbNumber')} style={{ marginBottom: 8 }}>
                <InputNumber value={redis.db ?? 0}
                  onChange={(db) => updateConfig({ db: db ?? 0 } as Partial<RedisConfig>)}
                  min={0} max={15} disabled={readOnly} style={{ width: 120 }} />
              </Form.Item>
              <Form.Item label="Topology Mode" style={{ marginBottom: 8 }}>
                <Select value={redis.topology?.mode || 'standalone'}
                  onChange={(mode) => updateConfig({
                    topology: mode === 'sentinel'
                      ? { mode, sentinel: { masterName: '', sentinels: [] } }
                      : mode === 'cluster'
                        ? { mode, cluster: { maxRedirects: 6 } }
                        : { mode },
                  } as Partial<RedisConfig>)}
                  disabled={readOnly}>
                  <Select.Option value="standalone">standalone</Select.Option>
                  <Select.Option value="sentinel">sentinel</Select.Option>
                  <Select.Option value="cluster">cluster</Select.Option>
                </Select>
              </Form.Item>
              {redis.topology?.mode === 'sentinel' && (
                <>
                  <Form.Item label="Sentinel Master Name" required style={{ marginBottom: 8 }}>
                    <Input value={redis.topology.sentinel?.masterName || ''}
                      onChange={(e) => updateConfig({ topology: {
                        ...redis.topology!,
                        sentinel: { ...(redis.topology?.sentinel || { masterName: '', sentinels: [] }), masterName: e.target.value },
                      } } as Partial<RedisConfig>)} disabled={readOnly} />
                  </Form.Item>
                  <Form.Item label="Sentinel Endpoints" required style={{ marginBottom: 0 }}>
                    <Select mode="tags" value={redis.topology.sentinel?.sentinels || []}
                      onChange={(sentinels) => updateConfig({ topology: {
                        ...redis.topology!,
                        sentinel: { ...(redis.topology?.sentinel || { masterName: '', sentinels: [] }), sentinels },
                      } } as Partial<RedisConfig>)} disabled={readOnly} />
                  </Form.Item>
                </>
              )}
            </Card>
          )
        })()}

        {type === 'elasticsearch' && (() => {
          const elasticsearch = config as ElasticsearchConfig
          return (
            <Card title={t('linksys.elasticsearch')} size="small">
              <Form.Item label={t('field.addresses')} required style={{ marginBottom: 8 }}>
                <Select mode="tags" value={elasticsearch.endpoints || []}
                  onChange={(endpoints) => updateConfig({ endpoints } as Partial<ElasticsearchConfig>)}
                  disabled={readOnly} placeholder="http://localhost:9200" style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item label={t('field.secretType')} style={{ marginBottom: 8 }}>
                <Select value={elasticsearch.auth?.type || 'basic'} disabled={readOnly}
                  onChange={(authType) => updateConfig({
                    auth: elasticsearch.auth
                      ? { ...elasticsearch.auth, type: authType }
                      : { type: authType, secretRef: { name: '' } },
                  } as Partial<ElasticsearchConfig>)}>
                  <Select.Option value="basic">basic</Select.Option>
                  <Select.Option value="apiKey">apiKey</Select.Option>
                  <Select.Option value="bearer">bearer</Select.Option>
                </Select>
              </Form.Item>
              <Form.Item label={t('field.secretName')} style={{ marginBottom: 0 }}>
                {renderSecretRef(elasticsearch.auth, (auth) => updateConfig({
                  auth: auth ? { ...auth, type: elasticsearch.auth?.type || 'basic' } : undefined,
                } as Partial<ElasticsearchConfig>))}
              </Form.Item>
            </Card>
          )
        })()}

        {type === 'etcd' && (() => {
          const etcd = config as EtcdConfig
          return (
            <Card title={t('linksys.etcd')} size="small">
              <Form.Item label={t('field.etcdEndpoints')} required style={{ marginBottom: 8 }}>
                <Select mode="tags" value={etcd.endpoints || []}
                  onChange={(endpoints) => updateConfig({ endpoints } as Partial<EtcdConfig>)}
                  disabled={readOnly} placeholder="http://localhost:2379" style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item label={t('field.secretName')} style={{ marginBottom: 0 }}>
                {renderSecretRef(etcd.auth, (auth) => updateConfig({ auth } as Partial<EtcdConfig>))}
              </Form.Item>
            </Card>
          )
        })()}

        {type === 'webhook' && (() => {
          const webhook = config as WebhookConfig
          return (
            <Card title={t('linksys.webhook')} size="small">
              <Form.Item label="URL" required style={{ marginBottom: 8 }}>
                <Input value={webhook.target?.url || ''}
                  onChange={(e) => updateConfig(withWebhookUrl(webhook, e.target.value))}
                  disabled={readOnly} placeholder="https://example.com" />
              </Form.Item>
              <Form.Item label={t('field.httpMethod')} style={{ marginBottom: 8 }}>
                <Select value={webhook.request?.method?.template || 'POST'}
                  onChange={(template) => updateConfig(withWebhookMethod(webhook, template))}
                  disabled={readOnly} style={{ width: 120 }}>
                  <Select.Option value="GET">GET</Select.Option>
                  <Select.Option value="POST">POST</Select.Option>
                  <Select.Option value="PUT">PUT</Select.Option>
                </Select>
              </Form.Item>
              <Form.Item label={t('field.timeoutMs')} style={{ marginBottom: 0 }}>
                <InputNumber value={webhook.timeoutMs ?? 5000}
                  onChange={(timeoutMs) => updateConfig({ timeoutMs: timeoutMs ?? 5000 } as Partial<WebhookConfig>)}
                  min={1} max={60000} disabled={readOnly} style={{ width: 160 }} />
              </Form.Item>
            </Card>
          )
        })()}

        {type === 'kafka' && (() => {
          const kafka = config as KafkaConfig
          return (
            <Card title="Kafka Config" size="small">
              <Form.Item label="Brokers" required style={{ marginBottom: 8 }}>
                <Select mode="tags" value={kafka.brokers || []}
                  onChange={(brokers) => updateConfig({ brokers } as Partial<KafkaConfig>)}
                  disabled={readOnly} placeholder="kafka:9092" style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item label="Channel Size" style={{ marginBottom: 8 }}>
                <InputNumber value={kafka.channelSize}
                  onChange={(channelSize) => updateConfig({ channelSize: channelSize ?? undefined } as Partial<KafkaConfig>)}
                  min={1} disabled={readOnly} style={{ width: 160 }} />
              </Form.Item>
              <Form.Item label="Linger (ms)" style={{ marginBottom: 0 }}>
                <InputNumber value={kafka.lingerMs}
                  onChange={(lingerMs) => updateConfig({ lingerMs: lingerMs ?? undefined } as Partial<KafkaConfig>)}
                  min={0} disabled={readOnly} style={{ width: 160 }} />
              </Form.Item>
            </Card>
          )
        })()}

        {type === 'httpdns' && (() => {
          const httpDns = config as HttpDnsConfig
          return (
            <Card title="HTTP DNS Config" size="small">
              <Form.Item label="Preset" style={{ marginBottom: 8 }}>
                <Select allowClear value={httpDns.preset}
                  onChange={(preset) => updateConfig({ preset } as Partial<HttpDnsConfig>)}
                  disabled={readOnly} placeholder="Custom">
                  <Select.Option value="aliyun">Aliyun</Select.Option>
                  <Select.Option value="tencent">Tencent</Select.Option>
                </Select>
              </Form.Item>
              <Form.Item label="URL Template" required={!httpDns.preset} style={{ marginBottom: 8 }}>
                <Input value={httpDns.urlTemplate || ''}
                  onChange={(e) => updateConfig({ urlTemplate: e.target.value } as Partial<HttpDnsConfig>)}
                  disabled={readOnly} placeholder="https://dns.example.com/resolve?host={domain}" />
              </Form.Item>
              <Form.Item label="Timeout (ms)" style={{ marginBottom: 0 }}>
                <InputNumber value={httpDns.connection?.timeoutMs}
                  onChange={(timeoutMs) => updateConfig({
                    connection: { ...httpDns.connection, timeoutMs: timeoutMs ?? undefined },
                  } as Partial<HttpDnsConfig>)}
                  min={1} disabled={readOnly} style={{ width: 160 }} />
              </Form.Item>
            </Card>
          )
        })()}
      </Space>
    </Form>
  )
}

export default LinkSysForm
