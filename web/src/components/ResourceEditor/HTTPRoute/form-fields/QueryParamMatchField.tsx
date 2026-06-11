/**
 * 查询参数匹配字段组件
 * 用于 HTTPRoute 的 matches[].queryParams 配置
 */

import React from 'react';
import { Select, Input, Space, Button } from 'antd';
import { MinusCircleOutlined } from '@ant-design/icons';
import { HEADER_MATCH_TYPES, DEFAULT_VALUES } from '@/constants/gateway-api';
import type { HTTPQueryParamMatch } from '@/types/gateway-api';

interface QueryParamMatchFieldProps {
  value?: HTTPQueryParamMatch;
  onChange?: (value: HTTPQueryParamMatch) => void;
  onRemove?: () => void;
  disabled?: boolean;
}

const QueryParamMatchField: React.FC<QueryParamMatchFieldProps> = ({
  value,
  onChange,
  onRemove,
  disabled = false,
}) => {
  const handleTypeChange = (type: string) => {
    onChange?.({
      ...value,
      type: type as HTTPQueryParamMatch['type'],
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
        placeholder="参数名称，如 page"
        disabled={disabled}
        style={{ width: 180 }}
      />
      <Input
        value={value?.value}
        onChange={handleValueChange}
        placeholder="参数值"
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

export default QueryParamMatchField;

