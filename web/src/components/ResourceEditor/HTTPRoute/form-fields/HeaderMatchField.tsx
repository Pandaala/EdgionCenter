/**
 * Header 匹配字段组件
 * 用于 HTTPRoute 的 matches[].headers 配置
 */

import React from 'react';
import { Select, Input, Space, Button } from 'antd';
import { MinusCircleOutlined } from '@ant-design/icons';
import { HEADER_MATCH_TYPES, DEFAULT_VALUES } from '@/constants/gateway-api';
import type { HTTPHeaderMatch } from '@/types/gateway-api';

interface HeaderMatchFieldProps {
  value?: HTTPHeaderMatch;
  onChange?: (value: HTTPHeaderMatch) => void;
  onRemove?: () => void;
  disabled?: boolean;
}

const HeaderMatchField: React.FC<HeaderMatchFieldProps> = ({
  value,
  onChange,
  onRemove,
  disabled = false,
}) => {
  const handleTypeChange = (type: string) => {
    onChange?.({
      ...value,
      type: type as HTTPHeaderMatch['type'],
      name: value?.name || '',
      value: value?.value || '',
    });
  };

  const handleNameChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    onChange?.({
      ...value,
      type: value?.type || DEFAULT_VALUES.matchType.type,
      name: e.target.value,
      value: value?.value || '',
    });
  };

  const handleValueChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    onChange?.({
      ...value,
      type: value?.type || DEFAULT_VALUES.matchType.type,
      name: value?.name || '',
      value: e.target.value,
    });
  };

  return (
    <Space align="baseline" style={{ width: '100%', marginBottom: 8 }}>
      <Select
        value={value?.type || DEFAULT_VALUES.matchType.type}
        onChange={handleTypeChange}
        disabled={disabled}
        style={{ width: 140 }}
      >
        {HEADER_MATCH_TYPES.map((type) => (
          <Select.Option key={type} value={type}>
            {type === 'Exact' && '精确 / Exact'}
            {type === 'RegularExpression' && '正则 / Regex'}
          </Select.Option>
        ))}
      </Select>
      <Input
        value={value?.name}
        onChange={handleNameChange}
        placeholder="Header 名称，如 X-Custom-Header"
        disabled={disabled}
        style={{ width: 220 }}
      />
      <Input
        value={value?.value}
        onChange={handleValueChange}
        placeholder="Header 值"
        disabled={disabled}
        style={{ flex: 1 }}
      />
      {onRemove && !disabled && (
        <Button
          type="text"
          danger
          icon={<MinusCircleOutlined />}
          onClick={onRemove}
        />
      )}
    </Space>
  );
};

export default HeaderMatchField;

