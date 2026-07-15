/**
 * StreamRoute 编辑器 Modal — 共享用于 TCPRoute / UDPRoute / TLSRoute
 */

import React, { useEffect, useState } from 'react'
import { Modal, Button, Tabs, message } from 'antd'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import StreamRouteForm from './StreamRouteForm'
import type { StreamRouteKind } from './StreamRouteForm'
import type { TCPRoute } from '@/types/gateway-api/tcproute'
import type { UDPRoute } from '@/types/gateway-api/udproute'
import type { TLSRoute } from '@/types/gateway-api/tlsroute'
import {
  createEmptyTCPRoute, normalizeTCPRoute, tcpRouteToYaml, yamlToTCPRoute,
} from '@/utils/tcproute'
import {
  createEmptyUDPRoute, normalizeUDPRoute, udpRouteToYaml, yamlToUDPRoute,
} from '@/utils/udproute'
import {
  createEmptyTLSRoute, normalizeTLSRoute, tlsRouteToYaml, yamlToTLSRoute,
} from '@/utils/tlsroute'
import type { ResourceKind } from '@/api/types'
import { useT } from '@/i18n'

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
}> = {
  TCPRoute: {
    apiKind: 'tcproute',
    label: 'TCPRoute',
    createEmpty: createEmptyTCPRoute,
    normalize: normalizeTCPRoute,
    toYaml: (r) => tcpRouteToYaml(r as TCPRoute),
    fromYaml: yamlToTCPRoute,
  },
  UDPRoute: {
    apiKind: 'udproute',
    label: 'UDPRoute',
    createEmpty: createEmptyUDPRoute,
    normalize: normalizeUDPRoute,
    toYaml: (r) => udpRouteToYaml(r as UDPRoute),
    fromYaml: yamlToUDPRoute,
  },
  TLSRoute: {
    apiKind: 'tlsroute',
    label: 'TLSRoute',
    createEmpty: createEmptyTLSRoute,
    normalize: normalizeTLSRoute,
    toYaml: (r) => tlsRouteToYaml(r as TLSRoute),
    fromYaml: yamlToTLSRoute,
  },
}

const StreamRouteEditor: React.FC<StreamRouteEditorProps> = ({
  visible, mode, kind, resource, onClose,
}) => {
  const t = useT()
  const [activeTab, setActiveTab] = useState<'form' | 'yaml'>('form')
  const [formData, setFormData] = useState<RouteData>(() => KIND_MAP[kind].createEmpty())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()

  const meta = KIND_MAP[kind]

  useEffect(() => {
    if (!visible) return
    setActiveTab('form')
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
  }, [visible, mode, resource, kind])

  const handleTabChange = (key: string) => {
    try {
      if (key === 'yaml') {
        setYamlContent(meta.toYaml(formData))
      } else {
        setFormData(meta.fromYaml(yamlContent))
      }
      setActiveTab(key as 'form' | 'yaml')
    } catch (e: any) {
      message.error(t('msg.tabSwitchFailed', { err: e.message }))
    }
  }

  const createMutation = useMutation({
    mutationFn: ({ namespace, yamlStr }: { namespace: string; yamlStr: string }) =>
      resourceApi.create(meta.apiKind, namespace, yamlStr),
    onSuccess: () => {
      message.success(t('msg.createOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', meta.apiKind] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.createFailed', { err: e.message })),
  })

  const updateMutation = useMutation({
    mutationFn: ({ namespace, name, yamlStr }: { namespace: string; name: string; yamlStr: string }) =>
      resourceApi.update(meta.apiKind, namespace, name, yamlStr),
    onSuccess: () => {
      message.success(t('msg.updateOk'))
      queryClient.invalidateQueries({ queryKey: ['resource-list', meta.apiKind] })
      onClose()
    },
    onError: (e: any) => message.error(t('msg.updateFailed', { err: e.message })),
  })

  const handleSubmit = () => {
    try {
      const yamlStr = activeTab === 'yaml' ? yamlContent : meta.toYaml(formData)
      const parsed = meta.fromYaml(yamlStr)
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
              <Button key="cancel" onClick={onClose}>{t('btn.cancel')}</Button>,
              <Button key="submit" type="primary" onClick={handleSubmit} loading={isPending}>
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
            label: t('tab.form'),
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
            label: t('tab.yaml'),
            children: (
              <YamlEditor
                value={yamlContent}
                onChange={setYamlContent}
                readOnly={isReadOnly}
                height="500px"
              />
            ),
          },
        ]}
      />
    </Modal>
  )
}

export default StreamRouteEditor
