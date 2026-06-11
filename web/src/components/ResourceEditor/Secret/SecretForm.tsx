/**
 * Secret 表单
 */

import React from 'react'
import { Form, Select, Input, Card, Button, Space } from 'antd'
import { PlusOutlined, MinusCircleOutlined } from '@ant-design/icons'
import MetadataSection from '../common/MetadataSection'
import type { SecretResource } from '@/utils/secret'
import { useT } from '@/i18n'

interface SecretFormProps {
  data: SecretResource
  onChange: (data: SecretResource) => void
  readOnly?: boolean
  isCreate?: boolean
}

const SECRET_TYPES = [
  { value: 'Opaque', label: 'Opaque' },
  { value: 'kubernetes.io/tls', label: 'kubernetes.io/tls' },
  { value: 'kubernetes.io/dockerconfigjson', label: 'kubernetes.io/dockerconfigjson' },
  { value: 'kubernetes.io/basic-auth', label: 'kubernetes.io/basic-auth' },
]

const SecretForm: React.FC<SecretFormProps> = ({ data, onChange, readOnly = false, isCreate = true }) => {
  const t = useT()
  const secretType = data.type || 'Opaque'
  const dataMap = data.data || {}

  const updateData = (newData: Record<string, string>) => {
    onChange({ ...data, data: newData })
  }

  const handleTypeChange = (val: string) => {
    // Reset data when switching type to avoid stale keys
    let newData: Record<string, string> = {}
    if (val === 'kubernetes.io/tls') {
      newData = { 'tls.crt': dataMap['tls.crt'] || '', 'tls.key': dataMap['tls.key'] || '' }
    } else if (val === 'kubernetes.io/basic-auth') {
      newData = { username: dataMap['username'] || '', password: dataMap['password'] || '' }
    }
    onChange({ ...data, type: val, data: newData })
  }

  const renderDataSection = () => {
    if (secretType === 'kubernetes.io/tls') {
      return (
        <>
          <Form.Item label="tls.crt" style={{ marginBottom: 8 }}>
            <Input.TextArea
              value={dataMap['tls.crt'] || ''}
              onChange={(e) => updateData({ ...dataMap, 'tls.crt': e.target.value })}
              placeholder={t('ph.dataValue')}
              disabled={readOnly}
              rows={4}
              style={{ fontFamily: 'monospace', fontSize: 12 }}
            />
          </Form.Item>
          <Form.Item label="tls.key" style={{ marginBottom: 0 }}>
            <Input.TextArea
              value={dataMap['tls.key'] || ''}
              onChange={(e) => updateData({ ...dataMap, 'tls.key': e.target.value })}
              placeholder={t('ph.dataValue')}
              disabled={readOnly}
              rows={4}
              style={{ fontFamily: 'monospace', fontSize: 12 }}
            />
          </Form.Item>
        </>
      )
    }

    if (secretType === 'kubernetes.io/basic-auth') {
      return (
        <>
          <Form.Item label={t('field.username')} style={{ marginBottom: 8 }}>
            <Input
              value={dataMap['username'] || ''}
              onChange={(e) => updateData({ ...dataMap, username: e.target.value })}
              placeholder={t('ph.dataValue')}
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item label={t('field.password')} style={{ marginBottom: 0 }}>
            <Input
              value={dataMap['password'] || ''}
              onChange={(e) => updateData({ ...dataMap, password: e.target.value })}
              placeholder={t('ph.dataValue')}
              disabled={readOnly}
            />
          </Form.Item>
        </>
      )
    }

    // Opaque / dockerconfigjson / generic: key-value editor
    const entries = Object.entries(dataMap)
    return (
      <>
        {entries.map(([k, v], idx) => (
          <Space key={`data-entry-${idx}`} style={{ display: 'flex', marginBottom: 8 }} align="start">
            <Input
              defaultValue={k}
              onBlur={(e) => {
                const newKey = e.target.value.trim()
                if (newKey !== k) {
                  const next: Record<string, string> = {}
                  for (const [ek, ev] of Object.entries(dataMap)) {
                    if (ek === k) {
                      if (newKey) next[newKey] = ev
                    } else {
                      next[ek] = ev
                    }
                  }
                  updateData(next)
                }
              }}
              placeholder={t('ph.dataKey')}
              disabled={readOnly}
              style={{ width: 160 }}
            />
            <Input.TextArea
              value={v}
              onChange={(e) => updateData({ ...dataMap, [k]: e.target.value })}
              placeholder={t('ph.dataValue')}
              disabled={readOnly}
              rows={2}
              style={{ width: 360, fontFamily: 'monospace', fontSize: 12 }}
            />
            {!readOnly && (
              <Button
                type="text"
                danger
                icon={<MinusCircleOutlined />}
                onClick={() => {
                  const next = { ...dataMap }
                  delete next[k]
                  updateData(next)
                }}
              />
            )}
          </Space>
        ))}
        {!readOnly && (
          <Button
            type="dashed"
            onClick={() => {
              const timestamp = Date.now()
              updateData({ ...dataMap, [`key-${timestamp}`]: '' })
            }}
            block
            icon={<PlusOutlined />}
          >
            {t('btn.addDataEntry')}
          </Button>
        )}
      </>
    )
  }

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata: { ...data.metadata, ...metadata, namespace: metadata.namespace || '' } })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        <Card title={t('section.secretType')} size="small">
          <Form.Item label={t('field.secretType')} style={{ marginBottom: 0 }}>
            <Select
              value={secretType}
              onChange={handleTypeChange}
              disabled={readOnly}
              style={{ width: 320 }}
              options={SECRET_TYPES}
            />
          </Form.Item>
        </Card>

        <Card title={t('section.secretData')} size="small">
          {renderDataSection()}
        </Card>
      </Space>
    </Form>
  )
}

export default SecretForm
