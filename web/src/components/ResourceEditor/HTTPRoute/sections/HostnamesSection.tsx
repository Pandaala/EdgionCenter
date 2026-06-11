/**
 * Hostnames Section - 主机名配置
 * 使用标签输入框支持多个主机名
 */

import React from 'react';
import { Card, Form, Select } from 'antd';
import type { Hostname } from '@/types/gateway-api';

interface HostnamesSectionProps {
  value?: Hostname[];
  onChange?: (value: Hostname[]) => void;
  disabled?: boolean;
}

const HostnamesSection: React.FC<HostnamesSectionProps> = ({
  value = [],
  onChange,
  disabled = false,
}) => {
  return (
    <Card title="主机名 / Hostnames" size="small">
      <Form.Item
        label="主机名列表（可选，支持通配符 *）"
        help="输入主机名后按回车添加，支持通配符前缀，如 *.example.com"
      >
        <Select
          mode="tags"
          value={value}
          onChange={onChange}
          placeholder="example.com, *.example.com"
          disabled={disabled}
          style={{ width: '100%' }}
        />
      </Form.Item>
    </Card>
  );
};

export default HostnamesSection;

