/**
 * HTTPRoute 主编辑器
 * 支持表单模式和 YAML 模式切换，带双向同步
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useState, useEffect } from 'react';
import { Modal, Tabs, Button, Space, message } from 'antd';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import HTTPRouteForm from './HTTPRouteForm';
import { editorCancelButtonProps, editorConditionsTab, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds';
import YamlEditor from '@/components/YamlEditor';
import ResourceConditions from '@/components/resource/ResourceConditions';
import { resourceApi } from '@/api/resources';
import { httpRouteSchema } from '@/schemas/gateway-api';
import {
  createEmptyHTTPRoute,
  normalizeHTTPRoute,
  httpRouteToYAML,
  yamlToHTTPRoute,
  DEFAULT_HTTPROUTE_YAML,
  toHTTPRouteMutationDocument,
} from '@/utils/httproute';
import { dumpYaml } from '@/utils/yaml-utils';
import type { HTTPRoute } from '@/types/gateway-api';
import { useT } from '@/i18n';
import { useEditorTabTransition } from '../useEditorTabTransition';


interface HTTPRouteEditorProps {
  visible: boolean;
  mode: 'create' | 'edit' | 'view';
  resource?: HTTPRoute | null;
  onClose: () => void;
}

const HTTPRouteEditor: React.FC<HTTPRouteEditorProps> = ({
  visible,
  mode: initialMode,
  resource,
  onClose,
}) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget();
  const [formData, setFormData] = useState<HTTPRoute | null>(null);
  const [yamlContent, setYamlContent] = useState<string>('');
  const queryClient = useQueryClient();
  const { activeTab, editableTab, resetEditorTab, handleTabChange } = useEditorTabTransition({
    formData,
    yamlContent,
    serialize: (value) => {
      if (!value) throw new Error(t('msg.formEmpty'))
      return httpRouteToYAML(value)
    },
    parse: (source) => {
      const parsed = yamlToHTTPRoute(source)
      httpRouteSchema.parse(parsed)
      return parsed
    },
    setFormData,
    setYamlContent,
    onError: (error) => message.error(t('msg.tabSwitchFailed', { err: error.message })),
  })
  const processedResource = useQuery({
    queryKey: [
      'processed-resource',
      mutationTarget.controllerId ?? 'direct',
      'httproute',
      resource?.metadata.namespace,
      resource?.metadata.name,
    ],
    queryFn: () => resourceApi.getProcessed<HTTPRoute>(
      mutationTarget,
      'httproute',
      resource?.metadata.namespace,
      resource!.metadata.name,
    ),
    enabled: visible && initialMode !== 'create' && Boolean(resource?.metadata.name),
    retry: 2,
    refetchInterval: visible && initialMode !== 'create' ? 2_000 : false,
  });

  // 是否只读（查看模式）
  const isReadOnly = initialMode === 'view';

  // 初始化数据
  useEffect(() => {
    if (visible) {
      if (resource) {
        // 编辑/查看模式：使用现有资源
        const normalized = normalizeHTTPRoute(resource);
        setFormData(normalized);
        setYamlContent(httpRouteToYAML(normalized));
      } else {
        // 创建模式：使用空模板
        const emptyRoute = createEmptyHTTPRoute();
        setFormData(emptyRoute);
        setYamlContent(DEFAULT_HTTPROUTE_YAML);
      }
      resetEditorTab();
    }
  }, [visible, initialMode, resource, resetEditorTab]);

  // 表单 → YAML 同步
  const handleFormChange = (newFormData: HTTPRoute) => {
    setFormData(newFormData);
    try {
      const yaml = httpRouteToYAML(newFormData);
      setYamlContent(yaml);
    } catch (e: any) {
      console.error('Form to YAML conversion error:', e);
    }
  };

  // 创建 Mutation
  const createMutation = useMutation({
    mutationFn: ({ namespace, content }: { namespace: string; name: string; content: string }) =>
      resourceApi.create(mutationTarget, 'httproute', namespace, content),
    onSuccess: () => {
      message.success(t('msg.createOk'));
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'httproute'] });
      onClose();
    },
    onError: (error: any) => {
      message.error(t('msg.createFailed', { err: error.message }));
    },
  });

  // 更新 Mutation
  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, content }: { namespace: string; name: string; content: string }) =>
      resourceApi.update(mutationTarget, 'httproute', namespace, name, content),
    onSuccess: () => {
      message.success(t('msg.updateOk'));
      queryClient.invalidateQueries({ queryKey: ['resource-list', 'httproute'] });
      onClose();
    },
    onError: (error: any) => {
      message.error(t('msg.updateFailed', { err: error.message }));
    },
  });

  // 提交处理
  const handleSubmit = async () => {
    try {
      let contentToSubmit: string;
      let parsedResource: HTTPRoute;

      if (editableTab === 'form') {
        // 表单模式：验证表单数据
        if (!formData) {
          message.error(t('msg.formEmpty'));
          return;
        }

        // Zod 验证
        httpRouteSchema.parse(formData);
        parsedResource = formData;
        contentToSubmit = dumpYaml(toHTTPRouteMutationDocument(formData, initialMode === 'create' ? 'create' : 'update'));
      } else {
        // YAML 模式：解析并验证 YAML
        parsedResource = yamlToHTTPRoute(yamlContent);
        httpRouteSchema.parse(parsedResource);
        contentToSubmit = dumpYaml(toHTTPRouteMutationDocument(parsedResource, initialMode === 'create' ? 'create' : 'update'));
      }

      const name = parsedResource.metadata?.name;
      const namespace = parsedResource.metadata?.namespace;

      if (!name || !namespace) {
        message.error(t('msg.metaRequired'));
        return;
      }

      if (initialMode === 'create') {
        createMutation.mutate({ namespace, name, content: contentToSubmit });
      } else if (resource) {
        if (name !== resource.metadata.name || namespace !== resource.metadata.namespace) {
          message.error(t('msg.noRename'));
          return;
        }
        updateMutation.mutate({ namespace, name, content: contentToSubmit });
      }
    } catch (e: any) {
      if (e.issues && Array.isArray(e.issues)) {
        // Zod 验证错误
        const errors = e.issues.map((issue: any) => issue.message).join('; ');
        message.error(t('msg.validationFailed', { err: errors }));
      } else {
        message.error(t('msg.submitFailed', { err: e.message || 'unknown error' }));
      }
    }
  };

  const title =
    initialMode === 'create'
      ? t('modal.create', { resource: 'HTTPRoute' })
      : initialMode === 'edit'
      ? t('modal.edit', { resource: resource?.metadata.name || 'HTTPRoute' })
      : t('modal.view', { resource: resource?.metadata.name || 'HTTPRoute' });

  const footer = (
    <Space>
      <Button {...editorCancelButtonProps} onClick={onClose}>
        {initialMode === 'view' ? t('btn.close') : t('btn.cancel')}
      </Button>
      {initialMode !== 'view' && (
        <Button
          {...editorSubmitButtonProps}
          type="primary"
          onClick={handleSubmit}
          loading={createMutation.isPending || updateMutation.isPending}
        >
          {initialMode === 'create' ? t('btn.create') : t('btn.save')}
        </Button>
      )}
    </Space>
  );

  return (
    <Modal
      title={title}
      open={visible}
      onCancel={onClose}
      footer={footer}
      width={1000}
      destroyOnClose
      style={{ top: 20 }}
    >
      <Tabs activeKey={activeTab} onChange={handleTabChange} items={[
        {
          key: 'form',
          label: editorFormTab(t('tab.form')),
          children: formData && (
            <HTTPRouteForm
              value={formData}
              onChange={handleFormChange}
              disabled={isReadOnly}
              isCreate={initialMode === 'create'}
            />
          ),
        },
        {
          key: 'yaml',
          label: editorYamlTab(t('tab.yaml')),
          children: (
            <YamlEditor
              value={yamlContent}
              onChange={setYamlContent}
              readOnly={isReadOnly}
              height="65vh"
            />
          ),
        },
        ...(initialMode !== 'create' ? [{
          key: 'conditions',
          label: editorConditionsTab(t('tab.conditions')),
          children: <ResourceConditions status={processedResource.data?.status ?? formData?.status} emptyText={t('status.noConditions')} />,
        }] : []),
      ]} />
    </Modal>
  );
};

export default HTTPRouteEditor;
