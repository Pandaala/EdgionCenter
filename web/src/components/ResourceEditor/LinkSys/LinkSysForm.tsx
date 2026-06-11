/**
 * LinkSys 表单 — 按 type 条件渲染配置区段
 */

import React from 'react'
import { Form, Input, InputNumber, Select, Switch, Card, Space } from 'antd'
import MetadataSection from '../common/MetadataSection'
import type { LinkSys, LinkSysType } from '@/types/link-sys'
import { useT } from '@/i18n'

interface LinkSysFormProps {
  data: LinkSys
  onChange: (data: LinkSys) => void
  readOnly?: boolean
  isCreate?: boolean
}

const LinkSysForm: React.FC<LinkSysFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()
  const type = data.spec?.type || 'redis'

  const updateSpec = (partial: Partial<typeof data.spec>) =>
    onChange({ ...data, spec: { ...data.spec, ...partial } })

  const updateTypeConfig = (key: string, partial: any) =>
    updateSpec({ [key]: { ...(data.spec as any)[key], ...partial } })

  const handleTypeChange = (newType: LinkSysType) => {
    const spec: any = { type: newType }
    if (newType === 'redis') spec.redis = { addresses: [], database: 0, clusterMode: false }
    if (newType === 'elasticsearch') spec.elasticsearch = { addresses: [] }
    if (newType === 'etcd') spec.etcd = { endpoints: [] }
    if (newType === 'webhook') spec.webhook = { url: '', method: 'POST' }
    onChange({ ...data, spec })
  }

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection value={data.metadata} onChange={(metadata) => onChange({ ...data, metadata })}
          disabled={readOnly} isCreate={isCreate} />

        <Card title={t('section.connType')} size="small">
          <Form.Item label={t('field.connType')} required style={{ marginBottom: 0 }}>
            <Select value={type} onChange={handleTypeChange} disabled={readOnly} style={{ width: 200 }}>
              <Select.Option value="redis">Redis</Select.Option>
              <Select.Option value="elasticsearch">Elasticsearch</Select.Option>
              <Select.Option value="etcd">etcd</Select.Option>
              <Select.Option value="webhook">Webhook</Select.Option>
            </Select>
          </Form.Item>
        </Card>

        {type === 'redis' && (
          <Card title={t('linksys.redis')} size="small">
            <Form.Item label={t('field.addresses')} style={{ marginBottom: 8 }}>
              <Select mode="tags" value={data.spec?.redis?.addresses || []}
                onChange={(v) => updateTypeConfig('redis', { addresses: v })}
                disabled={readOnly} placeholder="127.0.0.1:6379" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item label={t('field.password')} style={{ marginBottom: 8 }}>
              <Input.Password value={data.spec?.redis?.password || ''}
                onChange={(e) => updateTypeConfig('redis', { password: e.target.value })}
                disabled={readOnly} placeholder="redis-password" />
            </Form.Item>
            <Form.Item label={t('field.dbNumber')} style={{ marginBottom: 8 }}>
              <InputNumber value={data.spec?.redis?.database ?? 0}
                onChange={(v) => updateTypeConfig('redis', { database: v ?? 0 })}
                min={0} max={15} disabled={readOnly} style={{ width: 120 }} />
            </Form.Item>
            <Form.Item label={t('field.clusterMode')} style={{ marginBottom: 0 }}>
              <Switch checked={data.spec?.redis?.clusterMode || false}
                onChange={(v) => updateTypeConfig('redis', { clusterMode: v })}
                disabled={readOnly} />
            </Form.Item>
          </Card>
        )}

        {type === 'elasticsearch' && (
          <Card title={t('linksys.elasticsearch')} size="small">
            <Form.Item label={t('field.addresses')} style={{ marginBottom: 8 }}>
              <Select mode="tags" value={data.spec?.elasticsearch?.addresses || []}
                onChange={(v) => updateTypeConfig('elasticsearch', { addresses: v })}
                disabled={readOnly} placeholder="http://localhost:9200" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item label={t('field.username')} style={{ marginBottom: 8 }}>
              <Input value={data.spec?.elasticsearch?.username || ''}
                onChange={(e) => updateTypeConfig('elasticsearch', { username: e.target.value })}
                disabled={readOnly} placeholder="elastic" />
            </Form.Item>
            <Form.Item label={t('field.password')} style={{ marginBottom: 0 }}>
              <Input.Password value={data.spec?.elasticsearch?.password || ''}
                onChange={(e) => updateTypeConfig('elasticsearch', { password: e.target.value })}
                disabled={readOnly} />
            </Form.Item>
          </Card>
        )}

        {type === 'etcd' && (
          <Card title={t('linksys.etcd')} size="small">
            <Form.Item label={t('field.etcdEndpoints')} style={{ marginBottom: 8 }}>
              <Select mode="tags" value={data.spec?.etcd?.endpoints || []}
                onChange={(v) => updateTypeConfig('etcd', { endpoints: v })}
                disabled={readOnly} placeholder="http://localhost:2379" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item label={t('field.username')} style={{ marginBottom: 8 }}>
              <Input value={data.spec?.etcd?.username || ''}
                onChange={(e) => updateTypeConfig('etcd', { username: e.target.value })}
                disabled={readOnly} placeholder="etcd-user" />
            </Form.Item>
            <Form.Item label={t('field.password')} style={{ marginBottom: 0 }}>
              <Input.Password value={data.spec?.etcd?.password || ''}
                onChange={(e) => updateTypeConfig('etcd', { password: e.target.value })}
                disabled={readOnly} />
            </Form.Item>
          </Card>
        )}

        {type === 'webhook' && (
          <Card title={t('linksys.webhook')} size="small">
            <Form.Item label="URL" required style={{ marginBottom: 8 }}>
              <Input value={data.spec?.webhook?.url || ''}
                onChange={(e) => updateTypeConfig('webhook', { url: e.target.value })}
                disabled={readOnly} placeholder="https://example.com/webhook" />
            </Form.Item>
            <Form.Item label={t('field.httpMethod')} style={{ marginBottom: 8 }}>
              <Select value={data.spec?.webhook?.method || 'POST'}
                onChange={(v) => updateTypeConfig('webhook', { method: v })}
                disabled={readOnly} style={{ width: 120 }}>
                <Select.Option value="GET">GET</Select.Option>
                <Select.Option value="POST">POST</Select.Option>
                <Select.Option value="PUT">PUT</Select.Option>
              </Select>
            </Form.Item>
            <Form.Item label={t('field.timeoutMs')} style={{ marginBottom: 0 }}>
              <InputNumber value={data.spec?.webhook?.timeoutMs || 5000}
                onChange={(v) => updateTypeConfig('webhook', { timeoutMs: v ?? 5000 })}
                min={100} max={60000} disabled={readOnly} style={{ width: 160 }} />
            </Form.Item>
          </Card>
        )}
      </Space>
    </Form>
  )
}

export default LinkSysForm
