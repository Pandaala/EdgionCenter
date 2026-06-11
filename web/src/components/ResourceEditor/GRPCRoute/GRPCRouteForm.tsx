/**
 * GRPCRoute 表单
 */

import React from 'react'
import { Form, Space } from 'antd'
import MetadataSection from '../common/MetadataSection'
import ParentRefsSection from '../common/ParentRefsSection'
import HostnamesSection from '../common/HostnamesSection'
import GRPCRulesSection from './sections/GRPCRulesSection'
import type { GRPCRoute } from '@/types/gateway-api/grpcroute'

interface GRPCRouteFormProps {
  data: GRPCRoute
  onChange: (data: GRPCRoute) => void
  readOnly?: boolean
  isCreate?: boolean
}

const GRPCRouteForm: React.FC<GRPCRouteFormProps> = ({
  data, onChange, readOnly = false, isCreate = true,
}) => {
  const updateSpec = (partial: Partial<typeof data.spec>) =>
    onChange({ ...data, spec: { ...data.spec, ...partial } })

  return (
    <Form layout="vertical" size="small">
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        <ParentRefsSection
          value={data.spec?.parentRefs || []}
          onChange={(parentRefs) => updateSpec({ parentRefs })}
          disabled={readOnly}
          namespace={data.metadata?.namespace}
        />

        <HostnamesSection
          value={data.spec?.hostnames || []}
          onChange={(hostnames) => updateSpec({ hostnames })}
          disabled={readOnly}
        />

        <GRPCRulesSection
          value={data.spec?.rules || []}
          onChange={(rules) => updateSpec({ rules })}
          disabled={readOnly}
          namespace={data.metadata?.namespace}
        />
      </Space>
    </Form>
  )
}

export default GRPCRouteForm
