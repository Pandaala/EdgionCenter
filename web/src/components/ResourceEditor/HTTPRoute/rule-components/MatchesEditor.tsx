/**
 * 匹配条件编辑器
 * 组合 Path, Headers, QueryParams, Method
 */

import React from 'react';
import { Card, Form, Select, Button } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import PathMatchField from '../form-fields/PathMatchField';
import HeaderMatchField from '../form-fields/HeaderMatchField';
import QueryParamMatchField from '../form-fields/QueryParamMatchField';
import { HTTP_METHODS } from '@/constants/gateway-api';
import type { HTTPRouteMatch } from '@/types/gateway-api';

interface MatchesEditorProps {
  value?: HTTPRouteMatch[];
  onChange?: (value: HTTPRouteMatch[]) => void;
  disabled?: boolean;
}

const MatchesEditor: React.FC<MatchesEditorProps> = ({ value = [], onChange, disabled = false }) => {
  const handleMatchChange = (index: number, updatedMatch: HTTPRouteMatch) => {
    const newMatches = [...value];
    newMatches[index] = updatedMatch;
    onChange?.(newMatches);
  };

  const handleAddMatch = () => {
    onChange?.([
      ...value,
      {
        path: { type: 'PathPrefix', value: '/' },
      },
    ]);
  };

  const handleRemoveMatch = (index: number) => {
    const newMatches = value.filter((_, i) => i !== index);
    onChange?.(newMatches);
  };

  const handlePathChange = (index: number, path: HTTPRouteMatch['path']) => {
    handleMatchChange(index, { ...value[index], path });
  };

  const handleHeadersChange = (index: number, headers: HTTPRouteMatch['headers']) => {
    handleMatchChange(index, { ...value[index], headers });
  };

  const handleQueryParamsChange = (index: number, queryParams: HTTPRouteMatch['queryParams']) => {
    handleMatchChange(index, { ...value[index], queryParams });
  };

  const handleMethodChange = (index: number, method: HTTPRouteMatch['method']) => {
    handleMatchChange(index, { ...value[index], method });
  };

  return (
    <div>
      {value.map((match, index) => (
        <Card
          key={index}
          size="small"
          title={`匹配条件 ${index + 1}`}
          extra={
            !disabled && value.length > 1 && (
              <Button danger size="small" onClick={() => handleRemoveMatch(index)}>
                删除
              </Button>
            )
          }
          style={{ marginBottom: 16 }}
        >
          {/* 路径匹配 */}
          <Form.Item label="路径匹配 / Path">
            <PathMatchField
              value={match.path}
              onChange={(path) => handlePathChange(index, path)}
              disabled={disabled}
            />
          </Form.Item>

          {/* HTTP 方法 */}
          <Form.Item label="HTTP 方法 / Method">
            <Select
              value={match.method}
              onChange={(method) => handleMethodChange(index, method)}
              placeholder="不限（可选）"
              allowClear
              disabled={disabled}
            >
              {HTTP_METHODS.map((method) => (
                <Select.Option key={method} value={method}>
                  {method}
                </Select.Option>
              ))}
            </Select>
          </Form.Item>

          {/* Header 匹配 */}
          <Form.Item label="Header 匹配 / Headers（可选）">
            {(match.headers || []).map((header, headerIndex) => (
              <HeaderMatchField
                key={headerIndex}
                value={header}
                onChange={(newHeader) => {
                  const newHeaders = [...(match.headers || [])];
                  newHeaders[headerIndex] = newHeader;
                  handleHeadersChange(index, newHeaders);
                }}
                onRemove={() => {
                  const newHeaders = (match.headers || []).filter((_, i) => i !== headerIndex);
                  handleHeadersChange(index, newHeaders.length > 0 ? newHeaders : undefined);
                }}
                disabled={disabled}
              />
            ))}
            {!disabled && (
              <Button
                type="dashed"
                onClick={() => {
                  handleHeadersChange(index, [
                    ...(match.headers || []),
                    { type: 'Exact', name: '', value: '' },
                  ]);
                }}
                block
                icon={<PlusOutlined />}
              >
                添加 Header 匹配
              </Button>
            )}
          </Form.Item>

          {/* 查询参数匹配 */}
          <Form.Item label="查询参数匹配 / Query Params（可选）">
            {(match.queryParams || []).map((param, paramIndex) => (
              <QueryParamMatchField
                key={paramIndex}
                value={param}
                onChange={(newParam) => {
                  const newParams = [...(match.queryParams || [])];
                  newParams[paramIndex] = newParam;
                  handleQueryParamsChange(index, newParams);
                }}
                onRemove={() => {
                  const newParams = (match.queryParams || []).filter((_, i) => i !== paramIndex);
                  handleQueryParamsChange(index, newParams.length > 0 ? newParams : undefined);
                }}
                disabled={disabled}
              />
            ))}
            {!disabled && (
              <Button
                type="dashed"
                onClick={() => {
                  handleQueryParamsChange(index, [
                    ...(match.queryParams || []),
                    { type: 'Exact', name: '', value: '' },
                  ]);
                }}
                block
                icon={<PlusOutlined />}
              >
                添加查询参数匹配
              </Button>
            )}
          </Form.Item>
        </Card>
      ))}

      {!disabled && (
        <Button type="dashed" onClick={handleAddMatch} block icon={<PlusOutlined />}>
          添加匹配条件
        </Button>
      )}
    </div>
  );
};

export default MatchesEditor;

