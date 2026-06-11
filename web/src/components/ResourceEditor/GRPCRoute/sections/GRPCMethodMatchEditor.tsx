/**
 * gRPC Method 匹配编辑器
 * 编辑 GRPCRouteMatch 中的 method 字段
 */

import React from 'react'
import { Card, Form, Input, Select, Button, Space } from 'antd'
import { PlusOutlined, MinusCircleOutlined } from '@ant-design/icons'
import type { GRPCRouteMatch, GRPCHeaderMatch } from '@/types/gateway-api/grpcroute'
import { useT } from '@/i18n'

interface GRPCMethodMatchEditorProps {
  value?: GRPCRouteMatch[]
  onChange?: (value: GRPCRouteMatch[]) => void
  disabled?: boolean
}

const defaultMatch = (): GRPCRouteMatch => ({
  method: { type: 'Exact', service: '', method: '' },
  headers: [],
})

const GRPCMethodMatchEditor: React.FC<GRPCMethodMatchEditorProps> = ({
  value = [],
  onChange,
  disabled = false,
}) => {
  const t = useT()

  const updateMatch = (index: number, updated: GRPCRouteMatch) => {
    const next = [...value]
    next[index] = updated
    onChange?.(next)
  }

  const addMatch = () => onChange?.([...value, defaultMatch()])

  const removeMatch = (index: number) => {
    onChange?.(value.filter((_, i) => i !== index))
  }

  const addHeader = (matchIndex: number) => {
    const match = value[matchIndex]
    const headers: GRPCHeaderMatch[] = [...(match.headers || []), { type: 'Exact', name: '', value: '' }]
    updateMatch(matchIndex, { ...match, headers })
  }

  const updateHeader = (matchIndex: number, headerIndex: number, header: GRPCHeaderMatch) => {
    const match = value[matchIndex]
    const headers = [...(match.headers || [])]
    headers[headerIndex] = header
    updateMatch(matchIndex, { ...match, headers })
  }

  const removeHeader = (matchIndex: number, headerIndex: number) => {
    const match = value[matchIndex]
    const headers = (match.headers || []).filter((_, i) => i !== headerIndex)
    updateMatch(matchIndex, { ...match, headers })
  }

  return (
    <div>
      {value.map((match, matchIndex) => (
        <Card
          key={matchIndex}
          size="small"
          title={t('grpc.rule', { n: matchIndex + 1 })}
          extra={
            !disabled && value.length > 1 && (
              <Button danger size="small" icon={<MinusCircleOutlined />}
                onClick={() => removeMatch(matchIndex)}>{t('btn.delete')}</Button>
            )
          }
          style={{ marginBottom: 12 }}
        >
          <Form.Item label={t('field.matchType')} style={{ marginBottom: 8 }}>
            <Select
              value={match.method?.type || 'Exact'}
              onChange={(v) => updateMatch(matchIndex, { ...match, method: { ...match.method, type: v } })}
              disabled={disabled}
              style={{ width: 200 }}
            >
              <Select.Option value="Exact">Exact</Select.Option>
              <Select.Option value="RegularExpression">RegularExpression</Select.Option>
            </Select>
          </Form.Item>

          <Form.Item label={t('field.grpcService')} style={{ marginBottom: 8 }}
            help="e.g. mypackage.MyService">
            <Input
              value={match.method?.service || ''}
              onChange={(e) => updateMatch(matchIndex, {
                ...match, method: { ...match.method, service: e.target.value }
              })}
              placeholder="mypackage.MyService"
              disabled={disabled}
            />
          </Form.Item>

          <Form.Item label={t('field.grpcMethod')} style={{ marginBottom: 12 }}>
            <Input
              value={match.method?.method || ''}
              onChange={(e) => updateMatch(matchIndex, {
                ...match, method: { ...match.method, method: e.target.value }
              })}
              placeholder="GetItem"
              disabled={disabled}
            />
          </Form.Item>

          {/* Header 匹配 */}
          {(match.headers || []).map((header, hIdx) => (
            <Space key={hIdx} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
              <Select
                value={header.type || 'Exact'}
                onChange={(v) => updateHeader(matchIndex, hIdx, { ...header, type: v as any })}
                disabled={disabled}
                style={{ width: 130 }}
              >
                <Select.Option value="Exact">Exact</Select.Option>
                <Select.Option value="RegularExpression">Regex</Select.Option>
              </Select>
              <Input
                value={header.name}
                onChange={(e) => updateHeader(matchIndex, hIdx, { ...header, name: e.target.value })}
                placeholder="Header name"
                disabled={disabled}
                style={{ width: 180 }}
              />
              <Input
                value={header.value}
                onChange={(e) => updateHeader(matchIndex, hIdx, { ...header, value: e.target.value })}
                placeholder="Header value"
                disabled={disabled}
                style={{ width: 180 }}
              />
              {!disabled && (
                <Button type="text" danger icon={<MinusCircleOutlined />}
                  onClick={() => removeHeader(matchIndex, hIdx)} />
              )}
            </Space>
          ))}

          {!disabled && (
            <Button type="dashed" size="small" icon={<PlusOutlined />}
              onClick={() => addHeader(matchIndex)}>{t('btn.addHeaderMatch')}</Button>
          )}
        </Card>
      ))}

      {!disabled && (
        <Button type="dashed" onClick={addMatch} block icon={<PlusOutlined />}>
          {t('btn.addMatch')}
        </Button>
      )}
    </div>
  )
}

export default GRPCMethodMatchEditor
