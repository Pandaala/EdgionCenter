import { useRef, useState } from 'react'
import { message } from 'antd'
import { ThunderboltOutlined } from '@ant-design/icons'
import { useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useT } from '@/i18n'
import PermissionAwareButton from './PermissionAwareButton'

export default function AcmeTriggerButton({ resource }: { resource: K8sResource }) {
  const target = useControllerMutationTarget()
  const client = useQueryClient()
  const t = useT()
  const inFlight = useRef(false)
  const [pending, setPending] = useState(false)
  const trigger = async () => {
    if (inFlight.current || !resource.metadata.namespace) return
    inFlight.current = true
    setPending(true)
    try {
      const outcome = await resourceApi.triggerAcme(target, resource.metadata.namespace, resource.metadata.name)
      if (outcome === 'queued') {
        message.success(t('acme.trigger.queued'))
        void client.invalidateQueries({ queryKey: ['resource-list', 'edgionacme'] })
      } else if (outcome === 'unknown') message.warning(t('acme.trigger.unknown'))
      else message.error(t(`acme.trigger.${outcome}`))
    } finally {
      inFlight.current = false
      setPending(false)
    }
  }
  return <PermissionAwareButton data-testid="acme-trigger" operation="acme.trigger"
    size="small" icon={<ThunderboltOutlined />} loading={pending}
    disabled={pending || !resource.metadata.namespace} onClick={trigger}>
    {t('btn.trigger')}
  </PermissionAwareButton>
}
