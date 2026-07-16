import type { ComponentProps } from 'react'
import { Button, Tooltip } from 'antd'
import { useParams } from 'react-router-dom'
import type { CenterCapabilities } from '@/api/client'
import type {
  ControllerAccessDocument,
  ControllerAccessOperation,
  ControllerAccessResourceVerb,
  ResourceKind,
} from '@/api/types'
import { controllerKindFor, operationIsAllowed } from '@/api/access'
import { useControllerAccess } from '@/hooks/useControllerAccess'
import { useServerInfo } from '@/hooks/useServerInfo'
import { usePermissions } from '@/utils/permissions'
import { getActiveControllerId } from '@/utils/proxy'
import { resourceActionTestId, type ResourceAction } from './testIds'

type Capability = keyof CenterCapabilities

export interface ActionAvailabilityInput {
  centerLoading: boolean
  centerPermissions: string[]
  isControllerProxy: boolean
  requiredPermission?: string
  capabilities?: Partial<CenterCapabilities>
  requiredCapability?: Capability
  controllerAccessLoading?: boolean
  controllerAccess?: ControllerAccessDocument
  resourceKind?: ResourceKind
  resourceVerb?: ControllerAccessResourceVerb
  operation?: ControllerAccessOperation
  externallyDisabled?: boolean
  disabledReason?: string
}

export function resolveControllerAccessId(routeControllerId: string | undefined, activeControllerId: string | null): string | null {
  return routeControllerId?.replace(/~/g, '/') ?? activeControllerId
}

export function resolveActionAvailability(input: ActionAvailabilityInput): {
  disabled: boolean
  reason?: string
} {
  if (input.externallyDisabled) return { disabled: true, reason: input.disabledReason }
  if (input.centerLoading) return { disabled: true, reason: 'Authorization is loading' }
  if (input.requiredPermission && !input.centerPermissions.includes(input.requiredPermission)) {
    return { disabled: true, reason: `Missing permission: ${input.requiredPermission}` }
  }
  if (input.requiredCapability && input.capabilities?.[input.requiredCapability] !== true) {
    return { disabled: true, reason: `Server capability is unavailable: ${input.requiredCapability}` }
  }

  const needsControllerAccess = Boolean(input.operation || (input.resourceKind && input.resourceVerb))
  if (!needsControllerAccess) return { disabled: false }

  if (input.isControllerProxy && !input.centerPermissions.includes('proxy:access')) {
    return { disabled: true, reason: 'Missing permission: proxy:access' }
  }
  if (input.controllerAccessLoading) {
    return { disabled: true, reason: 'Controller authorization is loading' }
  }
  if (!input.controllerAccess) {
    return { disabled: true, reason: 'Controller access is unavailable; mutations are disabled' }
  }

  if (input.operation && !operationIsAllowed(input.controllerAccess, input.operation)) {
    return { disabled: true, reason: `Controller denies operation: ${input.operation}` }
  }
  if (input.resourceKind && input.resourceVerb) {
    const controllerKind = controllerKindFor(input.resourceKind)
    const allowed = input.controllerAccess.resources
      .find((row) => row.kind === controllerKind)
      ?.verbs.includes(input.resourceVerb) === true
    if (!allowed) {
      return {
        disabled: true,
        reason: `Controller denies ${input.resourceVerb} on ${controllerKind}`,
      }
    }
  }
  return { disabled: false }
}

export default function PermissionAwareButton({
  requiredPermission,
  requiredCapability,
  resourceKind,
  resourceVerb,
  operation,
  disabledReason,
  disabled,
  ...buttonProps
}: ComponentProps<typeof Button> & {
  requiredPermission?: string
  requiredCapability?: Capability
  resourceKind?: ResourceKind
  resourceVerb?: ControllerAccessResourceVerb
  operation?: ControllerAccessOperation
  disabledReason?: string
}) {
  const permissionState = usePermissions()
  const serverInfo = useServerInfo()
  const { controllerId: routeControllerId } = useParams<{ controllerId?: string }>()
  // The route parameter is reactive and available during render. The global
  // proxy target is installed by ControllerProxy's layout effect, so reading
  // only that value here can permanently capture null when all other button
  // dependencies are already cached and no later render occurs.
  const controllerId = resolveControllerAccessId(routeControllerId, getActiveControllerId())
  const needsControllerAccess = Boolean(operation || (resourceKind && resourceVerb))
  const controllerAccess = useControllerAccess(controllerId, needsControllerAccess)
  const availability = resolveActionAvailability({
    centerLoading: permissionState.loading,
    centerPermissions: permissionState.permissions,
    isControllerProxy: controllerId !== null,
    requiredPermission,
    capabilities: serverInfo.data?.data?.capabilities,
    requiredCapability,
    controllerAccessLoading: needsControllerAccess && controllerAccess.authorizationPending,
    controllerAccess: controllerAccess.data,
    resourceKind,
    resourceVerb,
    operation,
    externallyDisabled: disabled,
    disabledReason,
  })

  const inferredAction: ResourceAction | undefined = resourceVerb === 'list' || resourceVerb === 'list-keys'
    ? 'refresh'
    : resourceVerb === 'get'
      ? 'row-view'
      : resourceVerb === 'create'
        ? 'create'
        : resourceVerb === 'update'
          ? 'row-edit'
          : resourceVerb === 'delete'
            ? 'row-delete'
            : undefined
  const inferredTestId = resourceKind && inferredAction ? resourceActionTestId(resourceKind, inferredAction) : undefined
  const button = <Button data-testid={inferredTestId} {...buttonProps} disabled={availability.disabled} />
  return availability.reason ? <Tooltip title={availability.reason}>{button}</Tooltip> : button
}
