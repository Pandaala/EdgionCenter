/**
 * StreamRoute 编辑器 Modal — 共享用于 TCPRoute / UDPRoute / TLSRoute
 */

import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import ResourceConditions from '@/components/resource/ResourceConditions'
import StreamRouteForm from './StreamRouteForm'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import type { StreamRouteKind } from './StreamRouteForm'
import type { TCPRoute } from '@/types/gateway-api/tcproute'
import type { UDPRoute } from '@/types/gateway-api/udproute'
import type { TLSRoute } from '@/types/gateway-api/tlsroute'
import {
  createEmptyTCPRoute, normalizeTCPRoute, tcpRouteToMutationYaml, tcpRouteToYaml, yamlToTCPRoute,
} from '@/utils/tcproute'
import {
  createEmptyUDPRoute, normalizeUDPRoute, udpRouteToMutationYaml, udpRouteToYaml, yamlToUDPRoute,
} from '@/utils/udproute'
import {
  createEmptyTLSRoute, normalizeTLSRoute, tlsRouteToMutationYaml, tlsRouteToYaml, yamlToTLSRoute,
} from '@/utils/tlsroute'
import type { ResourceKind } from '@/api/types'
import { useT } from '@/i18n'
import { useEditorTabTransition } from '../useEditorTabTransition'

type RouteData = TCPRoute | UDPRoute | TLSRoute

interface StreamRouteEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  kind: StreamRouteKind
  resource?: RouteData | null
  onClose: () => void
}

const KIND_MAP: Record<StreamRouteKind, {
  apiKind: ResourceKind
  label: string
  createEmpty: () => RouteData
  normalize: (raw: any) => RouteData
  toYaml: (r: RouteData) => string
  fromYaml: (s: string) => RouteData
  toMutationYaml: (r: RouteData, mode: 'create' | 'update') => string
}> = {
  TCPRoute: {
    apiKind: 'tcproute',
    label: 'TCPRoute',
    createEmpty: createEmptyTCPRoute,
    normalize: normalizeTCPRoute,
    toYaml: (r) => tcpRouteToYaml(r as TCPRoute),
    fromYaml: yamlToTCPRoute,
    toMutationYaml: (r, mode) => tcpRouteToMutationYaml(r as TCPRoute, mode),
  },
  UDPRoute: {
    apiKind: 'udproute',
    label: 'UDPRoute',
    createEmpty: createEmptyUDPRoute,
    normalize: normalizeUDPRoute,
    toYaml: (r) => udpRouteToYaml(r as UDPRoute),
    fromYaml: yamlToUDPRoute,
    toMutationYaml: (r, mode) => udpRouteToMutationYaml(r as UDPRoute, mode),
  },
  TLSRoute: {
    apiKind: 'tlsroute',
    label: 'TLSRoute',
    createEmpty: createEmptyTLSRoute,
    normalize: normalizeTLSRoute,
    toYaml: (r) => tlsRouteToYaml(r as TLSRoute),
    fromYaml: yamlToTLSRoute,
    toMutationYaml: (r, mode) => tlsRouteToMutationYaml(r as TLSRoute, mode),
  },
}

const StreamRouteEditor: React.FC<StreamRouteEditorProps> = ({
  visible, mode, kind, resource, onClose,
}) => {
  const t = useT()
  const mutationTarget = useControllerMutationTarget()
  const [formData, setFormData] = useState<RouteData>(() => KIND_MAP[kind].createEmpty())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()

  const meta = KIND_MAP[kind]
  const { activeTab, editableTab, resetEditorTab, handleTabChange } = useEditorTabTransition({
    formData, yamlContent, serialize: meta.toYaml, parse: meta.fromYaml, setFormData, setYamlContent,
    onError: (error) => message.error(t('msg.tabSwitchFailed', { err: error.message })),
  })

  useEffect(() => {
    if (!visible) return
    resetEditorTab()
    if (mode === 'create') {
      const empty = meta.createEmpty()
      setFormData(empty)
      setYamlContent(meta.toYaml(empty))
    } else if (resource) {
      const normalized = meta.normalize(resource)
      setFormData(normalized)
      setYamlContent(meta.toYaml(normalized))
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, mode, resource, kind, resetEditorTab])

  const createMutation = useMutation({
    mutationFn: ({ namespace, yamlStr }: { namespace: string; yamlStr: string }) =>
      resourceApi.create(mutationTarget, meta.apiKind, namespace, yamlStr),
    onSuccess: () => {
      message.success(t('msg.createOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', meta.apiKind] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, yamlStr }: { namespace: string; name: string; yamlStr: string }) =>
      resourceApi.update(mutationTarget, meta.apiKind, namespace, name, yamlStr),
    onSuccess: () => {
      message.success(t('msg.updateOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', meta.apiKind] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const sourceYaml = editableTab === 'yaml' ? yamlContent : meta.toYaml(formData)
      const parsed = meta.fromYaml(sourceYaml)
      const name = parsed.metadata?.name
      const namespace = parsed.metadata?.namespace
      if (!name || !namespace) {
        message.error(t('msg.metaRequired'))
        return
      }
      if (mode !== 'create' && resource) {
        if (name !== resource.metadata.name || namespace !== resource.metadata.namespace) {
          message.error(t('msg.noRename'))
          return
        }
      }
      const yamlStr = meta.toMutationYaml(parsed, mode === 'create' ? 'create' : 'update')
      if (mode === 'create') {
        createMutation.mutate({ namespace, yamlStr })
      } else {
        updateMutation.mutate({ namespace, name, yamlStr })
      }
    } catch (e: any) {
      message.error(t('msg.submitFailed', { err: e.message || 'unknown error' }))
    }
  }

  const isPending = createMutation.isPending || updateMutation.isPending
  const isReadOnly = mode === 'view'

  const title =
    mode === 'create'
      ? t('modal.create', { resource: meta.label })
      : mode === 'edit'
      ? t('modal.edit', { resource: meta.label })
      : t('modal.view', { resource: meta.label })

  return (
    <Modal
      title={title}
      open={visible}
      onCancel={onClose}
      width={860}
      destroyOnClose
      style={{ top: 20 }}
      footer={
        isReadOnly
          ? [<Button key="close" onClick={onClose}>{t('btn.close')}</Button>]
          : [
              <Button {...editorCancelButtonProps} key="cancel" onClick={onClose}>{t('btn.cancel')}</Button>,
              <Button {...editorSubmitButtonProps} key="submit" type="primary" onClick={handleSubmit} loading={isPending}>
                {mode === 'create' ? t('btn.create') : t('btn.save')}
              </Button>,
            ]
      }
    >
      <Tabs
        activeKey={activeTab}
        onChange={handleTabChange}
        items={[
          {
            key: 'form',
            label: editorFormTab(t('tab.form')),
            children: (
              <StreamRouteForm
                kind={kind}
                data={formData}
                onChange={setFormData}
                readOnly={isReadOnly}
                isCreate={mode === 'create'}
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
                height="500px"
              />
            ),
          },
          ...(mode !== 'create' ? [{ key: 'conditions', label: t('tab.conditions'), children: <ResourceConditions status={formData.status} emptyText={t('status.noConditions')} /> }] : []),
        ]}
      />
    </Modal>
  )
}

export default StreamRouteEditor
