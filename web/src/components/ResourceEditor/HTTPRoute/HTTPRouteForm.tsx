/**
 * HTTPRoute 表单容器
 * 整合所有 Section 组件
 */

import React from 'react';
import { Form, Space } from 'antd';
import MetadataSection from './sections/MetadataSection';
import ParentRefsSection from './sections/ParentRefsSection';
import HostnamesSection from './sections/HostnamesSection';
import RulesSection from './sections/RulesSection';
import type { HTTPRoute } from '@/types/gateway-api';

interface HTTPRouteFormProps {
  value?: HTTPRoute;
  onChange?: (value: HTTPRoute) => void;
  disabled?: boolean;
  isCreate?: boolean;
}

const HTTPRouteForm: React.FC<HTTPRouteFormProps> = ({
  value,
  onChange,
  disabled = false,
  isCreate = true,
}) => {
  const handleMetadataChange = (metadata: HTTPRoute['metadata']) => {
    onChange?.({ ...value!, metadata });
  };

  const handleParentRefsChange = (parentRefs: HTTPRoute['spec']['parentRefs']) => {
    onChange?.({
      ...value!,
      spec: { ...value!.spec, parentRefs },
    });
  };

  const handleHostnamesChange = (hostnames: HTTPRoute['spec']['hostnames']) => {
    onChange?.({
      ...value!,
      spec: { ...value!.spec, hostnames },
    });
  };

  const handleRulesChange = (rules: HTTPRoute['spec']['rules']) => {
    onChange?.({
      ...value!,
      spec: { ...value!.spec, rules },
    });
  };

  return (
    <Form layout="vertical" style={{ maxHeight: '70vh', overflowY: 'auto', paddingRight: 16 }}>
      <Space direction="vertical" size="large" style={{ width: '100%' }}>
        {/* 基础信息 */}
        <MetadataSection
          value={value?.metadata}
          onChange={handleMetadataChange}
          disabled={disabled}
          isCreate={isCreate}
        />

        {/* Gateway 引用 */}
        <ParentRefsSection
          value={value?.spec.parentRefs}
          onChange={handleParentRefsChange}
          disabled={disabled}
          namespace={value?.metadata.namespace}
        />

        {/* 主机名 */}
        <HostnamesSection
          value={value?.spec.hostnames}
          onChange={handleHostnamesChange}
          disabled={disabled}
        />

        {/* 路由规则 */}
        <RulesSection
          value={value?.spec.rules}
          onChange={handleRulesChange}
          disabled={disabled}
          namespace={value?.metadata.namespace}
        />
      </Space>
    </Form>
  );
};

export default HTTPRouteForm;

