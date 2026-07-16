type ResourceDocument = { kind?: string; metadata?: Record<string, unknown>; spec?: Record<string, unknown>; [key: string]: unknown }

const SERVICE_IMMUTABLE_FIELDS = ['clusterIP', 'clusterIPs', 'ipFamilies', 'ipFamilyPolicy', 'healthCheckNodePort'] as const

export function buildCasReplacement(desired: ResourceDocument, current: ResourceDocument): ResourceDocument {
  const resourceVersion = current.metadata?.resourceVersion
  if (typeof resourceVersion !== 'string' || !resourceVersion) throw new Error('Current resourceVersion is required for CAS replacement')
  const replacement = structuredClone(desired)
  replacement.metadata = { ...replacement.metadata, resourceVersion }
  if (desired.kind === 'Service') {
    replacement.spec = { ...replacement.spec }
    for (const field of SERVICE_IMMUTABLE_FIELDS) {
      const value = current.spec?.[field]
      if (value !== undefined) replacement.spec[field] = structuredClone(value)
    }
  }
  return replacement
}
