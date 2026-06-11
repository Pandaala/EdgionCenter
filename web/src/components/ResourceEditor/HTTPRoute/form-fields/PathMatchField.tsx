/**
 * 路径匹配字段组件
 * 用于 HTTPRoute 的 matches[].path 配置
 */

import React from 'react';
import { Select, Input, Space } from 'antd';
import { PATH_MATCH_TYPES, DEFAULT_VALUES } from '@/constants/gateway-api';
import type { HTTPPathMatch } from '@/types/gateway-api';

interface PathMatchFieldProps {
  value?: HTTPPathMatch;
  onChange?: (value: HTTPPathMatch) => void;
  disabled?: boolean;
}

const PathMatchField: React.FC<PathMatchFieldProps> = ({ value, onChange, disabled = false }) => {
  const pathType = value?.type || DEFAULT_VALUES.pathMatch.type;
  const pathValue = value?.value || DEFAULT_VALUES.pathMatch.value;

  const handleTypeChange = (type: string) => {
    onChange?.({
      type: type as HTTPPathMatch['type'],
      value: pathValue,
    });
  };

  const handleValueChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    onChange?.({
      type: pathType,
      value: e.target.value,
    });
  };

  return (
    <Space.Compact block>
      <Select
        value={pathType}
        onChange={handleTypeChange}
        disabled={disabled}
        style={{ width: 180 }}
      >
        {PATH_MATCH_TYPES.map((type) => (
          <Select.Option key={type} value={type}>
            {type === 'Exact' && '精确匹配 / Exact'}
            {type === 'PathPrefix' && '前缀匹配 / PathPrefix'}
            {type === 'RegularExpression' && '正则匹配 / RegularExpression'}
          </Select.Option>
        ))}
      </Select>
      <Input
        value={pathValue}
        onChange={handleValueChange}
        placeholder="路径，如 / 或 /api"
        disabled={disabled}
        style={{ flex: 1 }}
      />
    </Space.Compact>
  );
};

export default PathMatchField;

