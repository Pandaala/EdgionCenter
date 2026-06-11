/**
 * 后端服务引用编辑器
 * 用于配置 HTTPRoute 的 backendRefs
 */

import React from 'react';
import { Form, Input, InputNumber, Button, Space, Card } from 'antd';
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons';
import { DEFAULT_VALUES, PORT_MIN, PORT_MAX, WEIGHT_MIN, WEIGHT_MAX } from '@/constants/gateway-api';
import type { BackendRef } from '@/types/gateway-api';

interface BackendRefsEditorProps {
  value?: BackendRef[];
  onChange?: (value: BackendRef[]) => void;
  disabled?: boolean;
  namespace?: string;
}

const BackendRefsEditor: React.FC<BackendRefsEditorProps> = ({
  value = [],
  onChange,
  disabled = false,
  namespace = DEFAULT_VALUES.defaultNamespace,
}) => {
  const handleBackendChange = (index: number, updatedBackend: BackendRef) => {
    const newBackends = [...value];
    newBackends[index] = updatedBackend;
    onChange?.(newBackends);
  };

  const handleAddBackend = () => {
    onChange?.([
      ...value,
      {
        group: DEFAULT_VALUES.backendRef.group,
        kind: DEFAULT_VALUES.backendRef.kind,
        namespace: namespace,
        name: '',
        port: 80,
        weight: DEFAULT_VALUES.backendRef.weight,
      },
    ]);
  };

  const handleRemoveBackend = (index: number) => {
    const newBackends = value.filter((_, i) => i !== index);
    onChange?.(newBackends);
  };

  return (
    <div>
      {value.map((backend, index) => (
        <Card
          key={index}
          size="small"
          title={`后端服务 ${index + 1}`}
          extra={
            !disabled && value.length > 1 && (
              <Button
                danger
                size="small"
                icon={<MinusCircleOutlined />}
                onClick={() => handleRemoveBackend(index)}
              >
                删除
              </Button>
            )
          }
          style={{ marginBottom: 16 }}
        >
          <Space direction="vertical" style={{ width: '100%' }} size="middle">
            {/* Service 名称 */}
            <Form.Item
              label="Service 名称 / Name"
              required
              style={{ marginBottom: 0 }}
            >
              <Input
                value={backend.name}
                onChange={(e) =>
                  handleBackendChange(index, { ...backend, name: e.target.value })
                }
                placeholder="example-service"
                disabled={disabled}
              />
            </Form.Item>

            {/* 端口号 */}
            <Form.Item
              label="端口号 / Port"
              style={{ marginBottom: 0 }}
            >
              <InputNumber
                value={backend.port}
                onChange={(port) =>
                  handleBackendChange(index, { ...backend, port: port || undefined })
                }
                placeholder="80"
                min={PORT_MIN}
                max={PORT_MAX}
                disabled={disabled}
                style={{ width: '100%' }}
              />
            </Form.Item>

            {/* 权重（流量分配） */}
            <Form.Item
              label="权重 / Weight（流量分配比例）"
              style={{ marginBottom: 0 }}
            >
              <InputNumber
                value={backend.weight ?? DEFAULT_VALUES.backendRef.weight}
                onChange={(weight) =>
                  handleBackendChange(index, { ...backend, weight: weight || undefined })
                }
                min={WEIGHT_MIN}
                max={WEIGHT_MAX}
                disabled={disabled}
                style={{ width: '100%' }}
              />
            </Form.Item>

            {/* 命名空间（可选） */}
            <Form.Item
              label="命名空间 / Namespace（可选，默认同 HTTPRoute）"
              style={{ marginBottom: 0 }}
            >
              <Input
                value={backend.namespace || namespace}
                onChange={(e) =>
                  handleBackendChange(index, { ...backend, namespace: e.target.value })
                }
                placeholder={namespace}
                disabled={disabled}
              />
            </Form.Item>

            {/* Group（高级） */}
            <Form.Item
              label="Group（高级，默认为空）"
              style={{ marginBottom: 0 }}
            >
              <Input
                value={backend.group || DEFAULT_VALUES.backendRef.group}
                onChange={(e) =>
                  handleBackendChange(index, { ...backend, group: e.target.value })
                }
                placeholder=""
                disabled={disabled}
              />
            </Form.Item>

            {/* Kind（高级） */}
            <Form.Item
              label="Kind（高级，默认 Service）"
              style={{ marginBottom: 0 }}
            >
              <Input
                value={backend.kind || DEFAULT_VALUES.backendRef.kind}
                onChange={(e) =>
                  handleBackendChange(index, { ...backend, kind: e.target.value })
                }
                placeholder="Service"
                disabled={disabled}
              />
            </Form.Item>
          </Space>
        </Card>
      ))}

      {!disabled && (
        <Button
          type="dashed"
          onClick={handleAddBackend}
          block
          icon={<PlusOutlined />}
        >
          添加后端服务
        </Button>
      )}
    </div>
  );
};

export default BackendRefsEditor;

