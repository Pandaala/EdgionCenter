/**
 * ParentRefs Section - Gateway 引用
 * 配置 HTTPRoute 绑定的 Gateway
 */

import React from 'react';
import { Card, Form, Input, InputNumber, Button, Space } from 'antd';
import { PlusOutlined, MinusCircleOutlined } from '@ant-design/icons';
import { DEFAULT_VALUES, PORT_MIN, PORT_MAX } from '@/constants/gateway-api';
import type { ParentReference } from '@/types/gateway-api';

interface ParentRefsSectionProps {
  value?: ParentReference[];
  onChange?: (value: ParentReference[]) => void;
  disabled?: boolean;
  namespace?: string;
}

const ParentRefsSection: React.FC<ParentRefsSectionProps> = ({
  value = [],
  onChange,
  disabled = false,
  namespace = DEFAULT_VALUES.defaultNamespace,
}) => {
  const handleParentChange = (index: number, updatedParent: ParentReference) => {
    const newParents = [...value];
    newParents[index] = updatedParent;
    onChange?.(newParents);
  };

  const handleAddParent = () => {
    onChange?.([
      ...value,
      {
        group: DEFAULT_VALUES.parentRef.group,
        kind: DEFAULT_VALUES.parentRef.kind,
        namespace: namespace,
        name: '',
      },
    ]);
  };

  const handleRemoveParent = (index: number) => {
    const newParents = value.filter((_, i) => i !== index);
    if (newParents.length > 0) {
      onChange?.(newParents);
    }
  };

  return (
    <Card title="Gateway 引用 / ParentRefs" size="small">
      {value.map((parent, index) => (
        <Card
          key={index}
          type="inner"
          size="small"
          title={`Gateway ${index + 1}`}
          extra={
            !disabled && value.length > 1 && (
              <Button
                danger
                size="small"
                icon={<MinusCircleOutlined />}
                onClick={() => handleRemoveParent(index)}
              >
                删除
              </Button>
            )
          }
          style={{ marginBottom: 16 }}
        >
          <Space direction="vertical" style={{ width: '100%' }} size="middle">
            {/* Gateway 名称 */}
            <Form.Item
              label="Gateway 名称 / Name"
              required
              style={{ marginBottom: 0 }}
            >
              <Input
                value={parent.name}
                onChange={(e) =>
                  handleParentChange(index, { ...parent, name: e.target.value })
                }
                placeholder="example-gateway"
                disabled={disabled}
              />
            </Form.Item>

            {/* Namespace */}
            <Form.Item
              label="命名空间 / Namespace（可选，默认同 HTTPRoute）"
              style={{ marginBottom: 0 }}
            >
              <Input
                value={parent.namespace || namespace}
                onChange={(e) =>
                  handleParentChange(index, { ...parent, namespace: e.target.value })
                }
                placeholder={namespace}
                disabled={disabled}
              />
            </Form.Item>

            {/* SectionName（Listener 名称） */}
            <Form.Item
              label="Listener 名称 / SectionName（可选）"
              style={{ marginBottom: 0 }}
            >
              <Input
                value={parent.sectionName}
                onChange={(e) =>
                  handleParentChange(index, { ...parent, sectionName: e.target.value })
                }
                placeholder="http-listener"
                disabled={disabled}
              />
            </Form.Item>

            {/* Port */}
            <Form.Item
              label="端口号 / Port（可选）"
              style={{ marginBottom: 0 }}
            >
              <InputNumber
                value={parent.port}
                onChange={(port) =>
                  handleParentChange(index, { ...parent, port: port || undefined })
                }
                placeholder="80"
                min={PORT_MIN}
                max={PORT_MAX}
                disabled={disabled}
                style={{ width: '100%' }}
              />
            </Form.Item>

            {/* Group（高级） */}
            <Form.Item
              label="Group（高级，默认 gateway.networking.k8s.io）"
              style={{ marginBottom: 0 }}
            >
              <Input
                value={parent.group || DEFAULT_VALUES.parentRef.group}
                onChange={(e) =>
                  handleParentChange(index, { ...parent, group: e.target.value })
                }
                placeholder={DEFAULT_VALUES.parentRef.group}
                disabled={disabled}
              />
            </Form.Item>

            {/* Kind（高级） */}
            <Form.Item
              label="Kind（高级，默认 Gateway）"
              style={{ marginBottom: 0 }}
            >
              <Input
                value={parent.kind || DEFAULT_VALUES.parentRef.kind}
                onChange={(e) =>
                  handleParentChange(index, { ...parent, kind: e.target.value })
                }
                placeholder={DEFAULT_VALUES.parentRef.kind}
                disabled={disabled}
              />
            </Form.Item>
          </Space>
        </Card>
      ))}

      {!disabled && (
        <Button
          type="dashed"
          onClick={handleAddParent}
          block
          icon={<PlusOutlined />}
        >
          添加 Gateway 引用
        </Button>
      )}
    </Card>
  );
};

export default ParentRefsSection;

