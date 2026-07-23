/**
 * HTTPRoute 表单容器
 * 整合所有 Section 组件
 */

import React from 'react';
import { Card, Form, InputNumber, Select, Space, Typography } from 'antd';
import MetadataSection from './sections/MetadataSection';
import ParentRefsSection from './sections/ParentRefsSection';
import HostnamesSection from './sections/HostnamesSection';
import RulesSection from './sections/RulesSection';
import type { HTTPRoute } from '@/types/gateway-api';
import {
  HTTPROUTE_MIRROR_TUNING_ANNOTATIONS,
  type HTTPRouteMirrorTuningField,
  withHTTPRouteMirrorTuningAnnotation,
} from '@/utils/httproute';
import { useT } from '@/i18n';

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
  const t = useT();

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

  const mirrorAnnotationValue = (field: HTTPRouteMirrorTuningField) =>
    value?.metadata.annotations?.[HTTPROUTE_MIRROR_TUNING_ANNOTATIONS[field]];

  const updateMirrorAnnotation = (field: HTTPRouteMirrorTuningField, next: string | undefined) => {
    if (value) onChange?.(withHTTPRouteMirrorTuningAnnotation(value, field, next));
  };

  const mirrorNumberField = (field: Exclude<HTTPRouteMirrorTuningField, 'mirrorLog'>) => {
    const raw = mirrorAnnotationValue(field);
    const numericValue = raw !== undefined && raw !== '' && Number.isFinite(Number(raw))
      ? Number(raw)
      : undefined;
    return (
      <Form.Item label={t(`routeFilter.${field}`)} style={{ marginBottom: 0 }}>
        <InputNumber
          aria-label={t(`routeFilter.${field}`)}
          min={0}
          precision={0}
          value={numericValue}
          onChange={(next) => updateMirrorAnnotation(field, next === null ? undefined : String(next))}
          disabled={disabled}
        />
      </Form.Item>
    );
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

        <Card size="small" title={t('routeMirror.tuningTitle')} data-testid="mirror-tuning-annotations">
          <Space direction="vertical" style={{ width: '100%' }}>
            <Typography.Text type="secondary">{t('routeMirror.tuningScope')}</Typography.Text>
            <Space wrap>
              {mirrorNumberField('connectTimeoutMs')}
              {mirrorNumberField('writeTimeoutMs')}
              {mirrorNumberField('channelFullTimeoutMs')}
              {mirrorNumberField('maxBufferedChunks')}
              {mirrorNumberField('maxConcurrent')}
              <Form.Item label={t('routeFilter.mirrorLog')} style={{ marginBottom: 0 }}>
                <Select
                  aria-label={t('routeFilter.mirrorLog')}
                  allowClear
                  value={mirrorAnnotationValue('mirrorLog')}
                  options={[
                    { value: 'true', label: t('routeMirror.enabled') },
                    { value: 'false', label: t('routeMirror.disabled') },
                  ]}
                  onChange={(next) => updateMirrorAnnotation('mirrorLog', next)}
                  disabled={disabled}
                  style={{ width: 120 }}
                />
              </Form.Item>
            </Space>
          </Space>
        </Card>

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
