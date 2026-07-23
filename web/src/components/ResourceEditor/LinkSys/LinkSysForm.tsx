import React, { useRef } from 'react'
import { Card, Form, Input, InputNumber, Select, Space, Switch } from 'antd'
import MetadataSection from '../common/MetadataSection'
import JsonValueField from '../common/JsonValueField'
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
import { createConfig, withWebhookMethod, withWebhookServiceTarget, withWebhookUrl } from '@/utils/linksys'
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
  const variantDrafts = useRef<Partial<Record<LinkSysType, LinkSysConfig>>>({})

  const updateConfig = (partial: Partial<LinkSysConfig>) =>
    onChange({ ...data, spec: { ...data.spec, config: { ...config, ...partial } as LinkSysConfig } })

  const updateSecretRef = (auth: SecretAuth | undefined, partial: { name?: string; namespace?: string }) => {
    const name = partial.name ?? auth?.secretRef.name ?? ''
    const namespace = partial.namespace ?? auth?.secretRef.namespace
    if (!name) return undefined
    return { secretRef: { name, namespace: namespace || undefined } }
  }

  const handleTypeChange = (newType: LinkSysType) => {
    variantDrafts.current[type] = structuredClone(config)
    const restored = variantDrafts.current[newType] ?? createConfig(newType)
    onChange({ ...data, spec: { ...data.spec, type: newType, config: structuredClone(restored) } })
  }

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
                      ? {
                          ...redis.topology,
                          mode,
                          sentinel: redis.topology?.sentinel || { masterName: '', sentinels: [] },
                        }
                      : mode === 'cluster'
                        ? {
                            ...redis.topology,
                            mode,
                            cluster: redis.topology?.cluster || { maxRedirects: 6 },
                          }
                        : { ...redis.topology, mode },
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
              {redis.topology?.mode === 'cluster' && <Space wrap><Form.Item label="Read from replicas"><Switch checked={redis.topology.cluster?.readFromReplicas} disabled={readOnly} onChange={(readFromReplicas)=>updateConfig({topology:{...redis.topology!,cluster:{...redis.topology?.cluster,readFromReplicas}}} as Partial<RedisConfig>)}/></Form.Item><Form.Item label="Max redirects"><InputNumber min={0} value={redis.topology.cluster?.maxRedirects} disabled={readOnly} onChange={(maxRedirects)=>updateConfig({topology:{...redis.topology!,cluster:{...redis.topology?.cluster,maxRedirects:maxRedirects??undefined}}} as Partial<RedisConfig>)}/></Form.Item></Space>}
              <Card size="small" title="Timeouts and pool"><Space wrap>{(['connect','read','write'] as const).map(key=><Form.Item key={key} label={`${key} timeout`}><InputNumber min={0} value={redis.timeout?.[key]} disabled={readOnly} onChange={(value)=>updateConfig({timeout:{...redis.timeout,[key]:value??undefined}} as Partial<RedisConfig>)}/></Form.Item>)}<Form.Item label="Pool size"><InputNumber min={1} value={redis.pool?.size} disabled={readOnly} onChange={(size)=>updateConfig({pool:{...redis.pool,size:size??undefined}} as Partial<RedisConfig>)}/></Form.Item><Form.Item label="Min idle"><InputNumber min={0} value={redis.pool?.minIdle} disabled={readOnly} onChange={(minIdle)=>updateConfig({pool:{...redis.pool,minIdle:minIdle??undefined}} as Partial<RedisConfig>)}/></Form.Item><Form.Item label="Max retries"><InputNumber min={0} value={redis.retry?.maxRetries} disabled={readOnly} onChange={(maxRetries)=>updateConfig({retry:{...redis.retry,maxRetries:maxRetries??undefined}} as Partial<RedisConfig>)}/></Form.Item></Space></Card>
              <Space wrap><Form.Item label="TLS enabled"><Switch checked={redis.tls?.enabled} disabled={readOnly} onChange={(enabled)=>updateConfig({tls:{...redis.tls,enabled}} as Partial<RedisConfig>)}/></Form.Item><Form.Item label="Verify TLS"><Switch checked={redis.tls?.verify} disabled={readOnly} onChange={(verify)=>updateConfig({tls:{...redis.tls,verify}} as Partial<RedisConfig>)}/></Form.Item><Form.Item label="Metrics"><Switch checked={redis.observability?.metrics?.enabled} disabled={readOnly} onChange={(enabled)=>updateConfig({observability:{...redis.observability,metrics:{...redis.observability?.metrics,enabled}}} as Partial<RedisConfig>)}/></Form.Item><Form.Item label="Logging"><Switch checked={redis.observability?.logging?.enabled} disabled={readOnly} onChange={(enabled)=>updateConfig({observability:{...redis.observability,logging:{...redis.observability?.logging,enabled}}} as Partial<RedisConfig>)}/></Form.Item></Space>
              <Card size="small" title="Advanced timeout, pool, retry/backoff, TLS certificates and observability"><JsonValueField readOnly={readOnly} value={{timeout:redis.timeout||{},pool:redis.pool||{},retry:redis.retry||{},tls:redis.tls||{},observability:redis.observability||{}}} onChange={(advanced)=>updateConfig(advanced as Partial<RedisConfig>)}/></Card>
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
              <Space wrap><Form.Item label="Connect timeout"><InputNumber min={0} value={elasticsearch.timeout?.connect} disabled={readOnly} onChange={(connect)=>updateConfig({timeout:{...elasticsearch.timeout,connect:connect??undefined}} as Partial<ElasticsearchConfig>)}/></Form.Item><Form.Item label="Request timeout"><InputNumber min={0} value={elasticsearch.timeout?.request} disabled={readOnly} onChange={(request)=>updateConfig({timeout:{...elasticsearch.timeout,request:request??undefined}} as Partial<ElasticsearchConfig>)}/></Form.Item><Form.Item label="Bulk batch size"><InputNumber min={1} value={elasticsearch.bulk?.batchSize} disabled={readOnly} onChange={(batchSize)=>updateConfig({bulk:{...elasticsearch.bulk,batchSize:batchSize??undefined}} as Partial<ElasticsearchConfig>)}/></Form.Item><Form.Item label="Bulk flush interval"><InputNumber min={0} value={elasticsearch.bulk?.flushInterval} disabled={readOnly} onChange={(flushInterval)=>updateConfig({bulk:{...elasticsearch.bulk,flushInterval:flushInterval??undefined}} as Partial<ElasticsearchConfig>)}/></Form.Item><Form.Item label="Index prefix"><Input value={elasticsearch.index?.prefix} disabled={readOnly} onChange={(e)=>updateConfig({index:{...elasticsearch.index,prefix:e.target.value}} as Partial<ElasticsearchConfig>)}/></Form.Item><Form.Item label="Date pattern"><Input value={elasticsearch.index?.datePattern} disabled={readOnly} onChange={(e)=>updateConfig({index:{...elasticsearch.index,datePattern:e.target.value}} as Partial<ElasticsearchConfig>)}/></Form.Item><Form.Item label="TLS"><Switch checked={elasticsearch.tls?.enabled} disabled={readOnly} onChange={(enabled)=>updateConfig({tls:{...elasticsearch.tls,enabled}} as Partial<ElasticsearchConfig>)}/></Form.Item></Space>
              <Card size="small" title="Advanced TLS, timeout, pool, bulk and index"><JsonValueField readOnly={readOnly} value={{tls:elasticsearch.tls||{},timeout:elasticsearch.timeout||{},pool:elasticsearch.pool||{},bulk:elasticsearch.bulk||{},index:elasticsearch.index||{}}} onChange={(advanced)=>updateConfig(advanced as Partial<ElasticsearchConfig>)}/></Card>
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
              <Space wrap><Form.Item label="Namespace"><Input value={etcd.namespace} disabled={readOnly} onChange={(e)=>updateConfig({namespace:e.target.value} as Partial<EtcdConfig>)}/></Form.Item>{(['dial','request','keepAlive'] as const).map(key=><Form.Item key={key} label={`${key} timeout`}><InputNumber min={0} value={etcd.timeout?.[key]} disabled={readOnly} onChange={(value)=>updateConfig({timeout:{...etcd.timeout,[key]:value??undefined}} as Partial<EtcdConfig>)}/></Form.Item>)}<Form.Item label="Auto sync interval"><InputNumber min={0} value={etcd.autoSyncInterval} disabled={readOnly} onChange={(autoSyncInterval)=>updateConfig({autoSyncInterval:autoSyncInterval??undefined} as Partial<EtcdConfig>)}/></Form.Item><Form.Item label="TLS"><Switch checked={etcd.tls?.enabled} disabled={readOnly} onChange={(enabled)=>updateConfig({tls:{...etcd.tls,enabled}} as Partial<EtcdConfig>)}/></Form.Item><Form.Item label="Reject old cluster"><Switch checked={etcd.rejectOldCluster} disabled={readOnly} onChange={(rejectOldCluster)=>updateConfig({rejectOldCluster} as Partial<EtcdConfig>)}/></Form.Item></Space>
              <Space wrap><Form.Item label="Max call send size"><InputNumber min={0} value={etcd.maxCallSendSize} disabled={readOnly} onChange={(maxCallSendSize)=>updateConfig({maxCallSendSize:maxCallSendSize??undefined} as Partial<EtcdConfig>)}/></Form.Item><Form.Item label="Max call receive size"><InputNumber min={0} value={etcd.maxCallRecvSize} disabled={readOnly} onChange={(maxCallRecvSize)=>updateConfig({maxCallRecvSize:maxCallRecvSize??undefined} as Partial<EtcdConfig>)}/></Form.Item><Form.Item label="User agent"><Input value={etcd.userAgent} disabled={readOnly} onChange={(e)=>updateConfig({userAgent:e.target.value} as Partial<EtcdConfig>)}/></Form.Item></Space>
              <Card size="small" title="Advanced TLS, timeouts, keep-alive and observability"><JsonValueField readOnly={readOnly} value={{tls:etcd.tls||{},timeout:etcd.timeout||{},keepAlive:etcd.keepAlive||{},observability:etcd.observability||{}}} onChange={(advanced)=>updateConfig(advanced as Partial<EtcdConfig>)}/></Card>
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
              <Card size="small" title="Service target"><Space wrap><Form.Item label="Group"><Input value={webhook.target?.group} disabled={readOnly} onChange={(e)=>updateConfig(withWebhookServiceTarget(webhook,{group:e.target.value}))}/></Form.Item><Form.Item label="Kind"><Input value={webhook.target?.kind} disabled={readOnly} onChange={(e)=>updateConfig(withWebhookServiceTarget(webhook,{kind:e.target.value}))}/></Form.Item><Form.Item label="Name"><Input value={webhook.target?.name} disabled={readOnly} onChange={(e)=>updateConfig(withWebhookServiceTarget(webhook,{name:e.target.value}))}/></Form.Item><Form.Item label="Namespace"><Input value={webhook.target?.namespace} disabled={readOnly} onChange={(e)=>updateConfig(withWebhookServiceTarget(webhook,{namespace:e.target.value}))}/></Form.Item><Form.Item label="Port"><InputNumber min={1} max={65535} value={webhook.target?.port} disabled={readOnly} onChange={(port)=>updateConfig(withWebhookServiceTarget(webhook,{port:port??undefined}))}/></Form.Item><Form.Item label="Block private (URL only)"><Switch checked={webhook.target?.blockPrivate} disabled={readOnly||Boolean(webhook.target?.name||webhook.target?.group||webhook.target?.kind||webhook.target?.namespace||webhook.target?.port)} onChange={(blockPrivate)=>updateConfig({target:{...webhook.target,blockPrivate}} as Partial<WebhookConfig>)}/></Form.Item></Space></Card>
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
              <Space wrap><Form.Item label="Timeout template"><Input value={webhook.timeoutMsTemplate} disabled={readOnly} onChange={(e)=>updateConfig({timeoutMsTemplate:e.target.value||undefined} as Partial<WebhookConfig>)}/></Form.Item><Form.Item label="Max response bytes"><InputNumber min={1} value={webhook.maxResponseBytes} disabled={readOnly} onChange={(maxResponseBytes)=>updateConfig({maxResponseBytes:maxResponseBytes??undefined} as Partial<WebhookConfig>)}/></Form.Item><Form.Item label="Status on error"><InputNumber min={200} max={599} value={webhook.statusOnError} disabled={readOnly} onChange={(statusOnError)=>updateConfig({statusOnError:statusOnError??undefined} as Partial<WebhookConfig>)}/></Form.Item><Form.Item label="TLS enabled"><Switch checked={webhook.tls?.enabled} disabled={readOnly} onChange={(enabled)=>updateConfig({tls:{...webhook.tls,enabled}} as Partial<WebhookConfig>)}/></Form.Item><Form.Item label="TLS verify"><Switch checked={webhook.tls?.verify} disabled={readOnly} onChange={(verify)=>updateConfig({tls:{...webhook.tls,verify}} as Partial<WebhookConfig>)}/></Form.Item></Space>
              <Card size="small" title="TLS validation and client certificate" style={{marginTop:8}}><JsonValueField readOnly={readOnly} value={webhook.tls||{}} onChange={(tls)=>updateConfig({tls} as Partial<WebhookConfig>)}/></Card>
              <Card size="small" title="Retry (maxRetries, retryDelayMs, retryOnTimeout, retryOnConnectError, retryOnStatus, backoffPolicy, maxDelayMs, jitter, honorRetryAfter, retryOnBodyFailure)" style={{marginTop:8}}><JsonValueField readOnly={readOnly} value={webhook.retry||{}} onChange={(retry)=>updateConfig({retry} as Partial<WebhookConfig>)}/></Card>
              <Card size="small" title="Rate limit (rate, windowSec)" style={{marginTop:8}}><JsonValueField readOnly={readOnly} value={webhook.rateLimit||{}} onChange={(rateLimit)=>updateConfig({rateLimit} as Partial<WebhookConfig>)}/></Card>
              <Card size="small" title="Health check (active and passive/backoff)" style={{marginTop:8}}><JsonValueField readOnly={readOnly} value={webhook.healthCheck||{}} onChange={(healthCheck)=>updateConfig({healthCheck} as Partial<WebhookConfig>)}/></Card>
              <Card size="small" title="Success (statusCodes and body predicates)" style={{marginTop:8}}><JsonValueField readOnly={readOnly} value={webhook.success||{}} onChange={(success)=>updateConfig({success} as Partial<WebhookConfig>)}/></Card>
              <Card size="small" title="Request path, method, args, headers, cookies and body" style={{marginTop:8}}><JsonValueField readOnly={readOnly} value={webhook.request||{}} onChange={(request)=>updateConfig({request} as Partial<WebhookConfig>)}/></Card>
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
              <Form.Item label="SASL username"><Input value={kafka.sasl?.username} disabled={readOnly} onChange={(e)=>updateConfig({sasl:{...kafka.sasl,username:e.target.value}} as Partial<KafkaConfig>)}/></Form.Item>
              <Form.Item label="SASL password Secret">{renderSecretRef(kafka.sasl?.password,(password)=>updateConfig({sasl:{...kafka.sasl,password}} as Partial<KafkaConfig>))}</Form.Item>
              <Form.Item label="TLS"><Switch checked={kafka.tls?.enabled} disabled={readOnly} onChange={(enabled)=>updateConfig({tls:{...kafka.tls,enabled}} as Partial<KafkaConfig>)}/></Form.Item>
              <Card size="small" title="Advanced SASL and TLS certificates"><JsonValueField readOnly={readOnly} value={{sasl:kafka.sasl||{},tls:kafka.tls||{}}} onChange={(advanced)=>updateConfig(advanced as Partial<KafkaConfig>)}/></Card>
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
              <Space wrap><Form.Item label="Response kind"><Select value={httpDns.response?.kind||'json'} disabled={readOnly} options={['json','delimited'].map(value=>({value}))} onChange={(kind)=>updateConfig({response:{...httpDns.response,kind}} as Partial<HttpDnsConfig>)}/></Form.Item><Form.Item label="IP path"><Input value={httpDns.response?.ipPath} disabled={readOnly} onChange={(e)=>updateConfig({response:{...httpDns.response,ipPath:e.target.value}} as Partial<HttpDnsConfig>)}/></Form.Item><Form.Item label="Delimiter"><Input value={httpDns.response?.delimiter} disabled={readOnly} onChange={(e)=>updateConfig({response:{...httpDns.response,delimiter:e.target.value}} as Partial<HttpDnsConfig>)}/></Form.Item><Form.Item label="TTL path"><Input value={httpDns.response?.ttlPath} disabled={readOnly} onChange={(e)=>updateConfig({response:{...httpDns.response,ttlPath:e.target.value}} as Partial<HttpDnsConfig>)}/></Form.Item><Form.Item label="Fallback"><Select value={httpDns.fallback?.type||'system'} disabled={readOnly} options={['system','none','dns'].map(value=>({value}))} onChange={(fallbackType)=>updateConfig({fallback:{...httpDns.fallback,type:fallbackType}} as Partial<HttpDnsConfig>)}/></Form.Item></Space>
              {httpDns.fallback?.type==='dns'&&<Form.Item label="Fallback DNS servers"><Select mode="tags" value={httpDns.fallback.servers||[]} disabled={readOnly} onChange={(servers)=>updateConfig({fallback:{...httpDns.fallback!,servers}} as Partial<HttpDnsConfig>)}/></Form.Item>}
              <Card size="small" title="Advanced response, fallback and connection TLS"><JsonValueField readOnly={readOnly} value={{response:httpDns.response||{},fallback:httpDns.fallback||{},connection:httpDns.connection||{}}} onChange={(advanced)=>updateConfig(advanced as Partial<HttpDnsConfig>)}/></Card>
            </Card>
          )
        })()}
      </Space>
    </Form>
  )
}

export default LinkSysForm
