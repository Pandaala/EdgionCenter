import { Tooltip, Typography } from 'antd'
import type { K8sResource, ResourceKind } from '@/api/types'
import { useRuntimeResourceStatus } from '@/hooks/useRuntimeResourceStatus'
import { useT } from '@/i18n'
import ResourceConditions from './ResourceConditions'

/** Source conditions take precedence; runtime observations remain display-only. */
export default function ResourceStatus({ kind, resource }: { kind: ResourceKind; resource: K8sResource }) {
  const t = useT()
  const runtime = useRuntimeResourceStatus(kind, resource)
  if (resource.status !== undefined && resource.status !== null) {
    return <ResourceConditions status={resource.status} generation={resource.metadata.generation} compact />
  }
  const note = runtime.isFetching ? t('status.runtimeLoading')
    : runtime.isError || !resource.metadata.resourceVersion ? t('status.runtimeUnavailable')
      : !runtime.data?.matches ? t('status.runtimePending') : undefined
  if (note) return <Typography.Text type="secondary" data-testid="runtime-status-unavailable">{note}</Typography.Text>
  return (
    <Tooltip title={t('status.runtimeSource')}>
      <span data-testid="runtime-status-observation">
        <ResourceConditions status={runtime.data?.status} generation={resource.metadata.generation} compact />
      </span>
    </Tooltip>
  )
}
