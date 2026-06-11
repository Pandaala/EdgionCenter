/**
 * StreamRoute 表单 — 共享用于 TCPRoute / UDPRoute / TLSRoute
 * TCPRoute: parentRefs + annotations + backendRefs
 * UDPRoute: parentRefs + backendRefs
 * TLSRoute: parentRefs + hostnames + backendRefs + annotations
 */

import React from 'react'
import { Form, Space } from 'antd'
import MetadataSection from '../common/MetadataSection'
import ParentRefsSection from '../common/ParentRefsSection'
import HostnamesSection from '../common/HostnamesSection'
import BackendRefsEditor from '../common/BackendRefsEditor'
import StreamAnnotationsSection from './StreamAnnotationsSection'

export type StreamRouteKind = 'TCPRoute' | 'UDPRoute' | 'TLSRoute'

interface StreamRouteFormProps {
  kind: StreamRouteKind
  data: any
  onChange: (data: any) => void
  readOnly?: boolean
  isCreate?: boolean
}

const StreamRouteForm: React.FC<StreamRouteFormProps> = ({
  kind,
  data,
  onChange,
  readOnly = false,
  isCreate = true,
}) => {
  const update = (path: string, value: any) => {
    if (path.startsWith('metadata.')) {
      const field = path.slice('metadata.'.length)
      onChange({ ...data, metadata: { ...data.metadata, [field]: value } })
    } else if (path.startsWith('spec.')) {
      const field = path.slice('spec.'.length)
      onChange({ ...data, spec: { ...data.spec, [field]: value } })
    } else if (path === 'metadata') {
      onChange({ ...data, metadata: value })
    } else if (path === 'spec') {
      onChange({ ...data, spec: value })
    }
  }

  const handleRulesChange = (backendRefs: any[]) => {
    const rules = data.spec?.rules || [{}]
    const newRules = [{ ...rules[0], backendRefs }]
    update('spec', { ...data.spec, rules: newRules })
  }

  const backendRefs = data.spec?.rules?.[0]?.backendRefs || []

  const showHostnames = kind === 'TLSRoute'
  const showAnnotations = kind === 'TCPRoute' || kind === 'TLSRoute'

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(meta) => update('metadata', meta)}
          disabled={readOnly}
          isCreate={isCreate}
        />

        {showAnnotations && (
          <StreamAnnotationsSection
            annotations={data.metadata?.annotations || {}}
            onChange={(annotations) =>
              onChange({ ...data, metadata: { ...data.metadata, annotations } })
            }
            disabled={readOnly}
          />
        )}

        <ParentRefsSection
          value={data.spec?.parentRefs || []}
          onChange={(refs) => update('spec', { ...data.spec, parentRefs: refs })}
          disabled={readOnly}
          namespace={data.metadata?.namespace}
        />

        {showHostnames && (
          <HostnamesSection
            value={data.spec?.hostnames || []}
            onChange={(hostnames) => update('spec', { ...data.spec, hostnames })}
            disabled={readOnly}
          />
        )}

        <BackendRefsEditor
          value={backendRefs}
          onChange={handleRulesChange}
          disabled={readOnly}
          namespace={data.metadata?.namespace}
        />
      </Space>
    </Form>
  )
}

export default StreamRouteForm
