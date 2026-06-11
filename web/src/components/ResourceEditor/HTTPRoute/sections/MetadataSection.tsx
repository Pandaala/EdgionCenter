/**
 * Metadata Section - 基础信息
 * 包含 name, namespace, labels, annotations
 */

import React from 'react';
import { Card, Form, Input, Button, Space, Typography } from 'antd';
import { PlusOutlined, MinusCircleOutlined } from '@ant-design/icons';
import { DEFAULT_VALUES } from '@/constants/gateway-api';
import type { K8sObjectMeta } from '@/types/gateway-api';

const { Text } = Typography;

interface MetadataSectionProps {
  value?: K8sObjectMeta;
  onChange?: (value: K8sObjectMeta) => void;
  disabled?: boolean;
  isCreate?: boolean;
}

const MetadataSection: React.FC<MetadataSectionProps> = ({
  value = {
    name: '',
    namespace: DEFAULT_VALUES.defaultNamespace,
    labels: {},
    annotations: {},
  },
  onChange,
  disabled = false,
  isCreate = true,
}) => {
  const handleChange = (field: keyof K8sObjectMeta, fieldValue: any) => {
    onChange?.({ ...value, [field]: fieldValue });
  };

  return (
    <Card title="基础信息 / Basic Info" size="small">
      {/* 名称 */}
      <Form.Item
        label="名称 / Name"
        required
        rules={[{ required: true, message: '请输入资源名称' }]}
      >
        <Input
          value={value.name}
          onChange={(e) => handleChange('name', e.target.value)}
          placeholder="example-route"
          disabled={disabled || !isCreate}
        />
      </Form.Item>

      {/* 命名空间 */}
      <Form.Item
        label="命名空间 / Namespace"
        required
        rules={[{ required: true, message: '请选择命名空间' }]}
      >
        <Input
          value={value.namespace}
          onChange={(e) => handleChange('namespace', e.target.value)}
          placeholder="default"
          disabled={disabled || !isCreate}
        />
      </Form.Item>

      {/* Labels */}
      <Form.Item label="Labels（可选）">
        {Object.entries(value.labels || {}).map(([labelKey, labelValue]) => (
          <Space key={`label-${labelKey}`} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
            <Input
              defaultValue={labelKey}
              onBlur={(e) => {
                const newKey = e.target.value.trim();
                if (newKey !== labelKey) {
                  const newLabels = { ...(value.labels || {}) };
                  delete newLabels[labelKey];
                  if (newKey) {
                    newLabels[newKey] = labelValue as string;
                  }
                  handleChange('labels', newLabels);
                }
              }}
              placeholder="key"
              disabled={disabled}
              style={{ width: 200 }}
            />
            <Text>:</Text>
            <Input
              value={labelValue as string}
              onChange={(e) => {
                const newLabels = { ...(value.labels || {}) };
                newLabels[labelKey] = e.target.value;
                handleChange('labels', newLabels);
              }}
              placeholder="value"
              disabled={disabled}
              style={{ flex: 1, minWidth: 200 }}
            />
            {!disabled && (
              <Button
                type="text"
                danger
                icon={<MinusCircleOutlined />}
                onClick={() => {
                  const newLabels = { ...(value.labels || {}) };
                  delete newLabels[labelKey];
                  handleChange('labels', newLabels);
                }}
              />
            )}
          </Space>
        ))}
        {!disabled && (
          <Button
            type="dashed"
            onClick={() => {
              const timestamp = Date.now();
              const newLabels = { ...(value.labels || {}), [`new-key-${timestamp}`]: '' };
              handleChange('labels', newLabels);
            }}
            block
            icon={<PlusOutlined />}
          >
            添加 Label
          </Button>
        )}
      </Form.Item>

      {/* Annotations */}
      <Form.Item label="Annotations（可选）">
        {Object.entries(value.annotations || {}).map(([annotationKey, annotationValue]) => (
          <Space key={`annotation-${annotationKey}`} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
            <Input
              defaultValue={annotationKey}
              onBlur={(e) => {
                const newKey = e.target.value.trim();
                if (newKey !== annotationKey) {
                  const newAnnotations = { ...(value.annotations || {}) };
                  delete newAnnotations[annotationKey];
                  if (newKey) {
                    newAnnotations[newKey] = annotationValue as string;
                  }
                  handleChange('annotations', newAnnotations);
                }
              }}
              placeholder="key"
              disabled={disabled}
              style={{ width: 200 }}
            />
            <Text>:</Text>
            <Input
              value={annotationValue as string}
              onChange={(e) => {
                const newAnnotations = { ...(value.annotations || {}) };
                newAnnotations[annotationKey] = e.target.value;
                handleChange('annotations', newAnnotations);
              }}
              placeholder="value"
              disabled={disabled}
              style={{ flex: 1, minWidth: 200 }}
            />
            {!disabled && (
              <Button
                type="text"
                danger
                icon={<MinusCircleOutlined />}
                onClick={() => {
                  const newAnnotations = { ...(value.annotations || {}) };
                  delete newAnnotations[annotationKey];
                  handleChange('annotations', newAnnotations);
                }}
              />
            )}
          </Space>
        ))}
        {!disabled && (
          <Button
            type="dashed"
            onClick={() => {
              const timestamp = Date.now();
              const newAnnotations = { ...(value.annotations || {}), [`new-annotation-${timestamp}`]: '' };
              handleChange('annotations', newAnnotations);
            }}
            block
            icon={<PlusOutlined />}
          >
            添加 Annotation
          </Button>
        )}
      </Form.Item>
    </Card>
  );
};

export default MetadataSection;

