export interface ResourceSnapshot { generation?: number; resourceVersion?: string; spec: unknown; conditions: unknown[] }

export function collectConditions(status: any): unknown[] {
  if (!status || typeof status !== 'object' || Array.isArray(status)) return []
  if ('controllers' in status) {
    if (!Array.isArray(status.controllers)) return []
    return status.controllers.flatMap((entry: any) => {
      if (!entry || typeof entry.controllerName !== 'string' || !entry.controllerName) return []
      return collectNativeConditions(entry.status)
    })
  }
  return collectNativeConditions(status)
}

function collectNativeConditions(status: any): unknown[] {
  return [
    ...(Array.isArray(status?.conditions) ? status.conditions : []),
    ...(Array.isArray(status?.parents) ? status.parents.flatMap((parent: any) => Array.isArray(parent?.conditions) ? parent.conditions : []) : []),
    ...(Array.isArray(status?.listeners) ? status.listeners.flatMap((listener: any) => Array.isArray(listener?.conditions) ? listener.conditions : []) : []),
    ...(Array.isArray(status?.ancestors) ? status.ancestors.flatMap((ancestor: any) => Array.isArray(ancestor?.conditions) ? ancestor.conditions : []) : []),
  ]
}

/** A reported generation must match before a condition can prove convergence. */
export function hasCurrentCondition(
  snapshot: ResourceSnapshot,
  expected: { type: string; status: string; reason?: string },
): boolean {
  return snapshot.conditions.some((value) => {
    if (!value || typeof value !== 'object') return false
    const condition = value as Record<string, unknown>
    if (snapshot.generation !== undefined && (
      !Number.isSafeInteger(snapshot.generation) || snapshot.generation < 0
      || condition.observedGeneration !== snapshot.generation
    )) return false
    return condition.type === expected.type && condition.status === expected.status
      && (!expected.reason || condition.reason === expected.reason)
  })
}
