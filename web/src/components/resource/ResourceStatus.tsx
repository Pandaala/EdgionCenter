import { Tooltip, Typography } from 'antd'
import type { ReactNode } from 'react'
import type { K8sResource, ResourceKind } from '@/api/types'
import { useRuntimeResourceStatus } from '@/hooks/useRuntimeResourceStatus'
import { useT } from '@/i18n'
import ResourceConditions from './ResourceConditions'

/** Source conditions take precedence; runtime observations remain display-only. */
export default function ResourceStatus({ kind, resource, renderStatus }: {
  kind: ResourceKind
  resource: K8sResource
  renderStatus?: (status: K8sResource['status']) => ReactNode
}) {
  const t = useT()
  const runtime = useRuntimeResourceStatus(kind, resource)
  const render = renderStatus ?? ((status) => <ResourceConditions status={status} generation={resource.metadata.generation} compact />)
  if (resource.status !== undefined && resource.status !== null) {
    return <>{render(resource.status)}</>
  }
  const note = runtime.isFetching ? t('status.runtimeLoading')
    : runtime.isError || !resource.metadata.resourceVersion ? t('status.runtimeUnavailable')
      : !runtime.data?.matches ? t('status.runtimePending') : undefined
  if (note) return <Typography.Text type="secondary" data-testid="runtime-status-unavailable">{note}</Typography.Text>
  return (
    <Tooltip title={t('status.runtimeSource')}>
      <span data-testid="runtime-status-observation">
        {render(runtime.data?.status)}
      </span>
    </Tooltip>
  )
}
