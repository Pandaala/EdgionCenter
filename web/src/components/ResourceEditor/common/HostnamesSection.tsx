/**
 * Hostnames Section - 主机名配置
 * 使用标签输入框支持多个主机名
 */

import React from 'react';
import { Card, Form, Select } from 'antd';
import type { Hostname } from '@/types/gateway-api';
import { useT } from '@/i18n';

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
  const t = useT();

  return (
    <Card title={t('section.hostnames')} size="small">
      <Form.Item
        label={t('field.hosts')}
        help="Press Enter to add hostnames. Supports wildcard prefix, e.g. *.example.com"
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
