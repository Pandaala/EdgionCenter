import type { JsonValue } from '@/api/globalResources'
import type { GlobalResourceComparisonGroup } from '@/api/globalResources'

export type GlobalResourceConsistency = 'single' | 'consistent' | 'inconsistent'

const VOLATILE_METADATA_FIELDS = new Set([
  'resourceVersion',
  'uid',
  'generation',
  'creationTimestamp',
  'managedFields',
])

function normalizeValue(value: JsonValue): JsonValue {
  if (Array.isArray(value)) {
    return value.map((item) => normalizeValue(item))
  }
  if (value === null || typeof value !== 'object') {
    return value
  }

  return Object.fromEntries(
    Object.keys(value)
      .sort()
      .map((key) => [key, normalizeValue(value[key])]),
  )
}

export function normalizeGlobalResource(value: JsonValue): JsonValue {
  if (value === null || Array.isArray(value) || typeof value !== 'object') {
    return normalizeValue(value)
  }
  return Object.fromEntries(
    Object.keys(value)
      .filter((key) => key !== 'status')
      .sort()
      .map((key) => {
        if (key !== 'metadata') return [key, normalizeValue(value[key])]
        const metadata = value[key]
        if (metadata === null || Array.isArray(metadata) || typeof metadata !== 'object') {
          return [key, normalizeValue(metadata)]
        }
        return [
          key,
          Object.fromEntries(
            Object.keys(metadata)
              .filter((field) => !VOLATILE_METADATA_FIELDS.has(field))
              .sort()
              .map((field) => [field, normalizeValue(metadata[field])]),
          ),
        ]
      }),
  )
}

export function getGlobalResourceConsistency(
  group: GlobalResourceComparisonGroup,
): GlobalResourceConsistency {
  if (group.members.length < 2) {
    return 'single'
  }

  const baseline = JSON.stringify(normalizeGlobalResource(group.members[0].object))
  const consistent = group.members
    .slice(1)
    .every((member) => JSON.stringify(normalizeGlobalResource(member.object)) === baseline)
  return consistent ? 'consistent' : 'inconsistent'
}
