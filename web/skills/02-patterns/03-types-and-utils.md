---
name: types-and-utils-pattern
description: Standard patterns for TypeScript type definitions and YAML utility functions
---

# Type Definitions & Utility Functions Pattern

## Type Definition Pattern

File location: `src/types/{resource}/index.ts`

Reference:
- `src/types/gateway-api/httproute.ts` (305 lines)
- `src/types/edgion-plugins/index.ts` (156 lines)

### Standard Structure

```typescript
// 1. Enums / union types
export type PathMatchType = 'Exact' | 'PathPrefix' | 'RegularExpression'

// 2. Sub-types (defined leaf-to-root)
export interface SomeMatch {
  type?: MatchType
  value: string
}

// 3. Rule / Spec type
export interface ResourceSpec {
  // fields match the backend YAML Schema
}

// 4. Primary resource type
export interface ResourceType {
  apiVersion: string   // e.g., 'gateway.networking.k8s.io/v1'
  kind: string         // e.g., 'HTTPRoute'
  metadata: K8sMetadata
  spec: ResourceSpec
  status?: any
}
```

### Key Points
- Type names match K8s resource names (PascalCase)
- Optional fields are marked with `?`
- Reuse `K8sMetadata` and `K8sResource` (from `src/api/types.ts`)
- Use string literal types or `string` for apiVersion and kind

## Utility Function Pattern

File location: `src/utils/{resource}.ts`

Reference:
- `src/utils/httproute.ts` (190 lines)
- `src/utils/edgionplugins.ts` (129 lines)

### Required Functions

```typescript
import * as yaml from 'js-yaml'
import type { ResourceType } from '@/types/{resource}'

/**
 * Create an empty resource object (used in create mode)
 */
export function createEmptyResource(): ResourceType {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'ResourceName',
    metadata: {
      name: '',
      namespace: 'default',
    },
    spec: {
      // default values for all required fields
    },
  }
}

/**
 * Normalize data returned from the backend (fill in missing fields, unify format)
 */
export function normalizeResource(raw: any): ResourceType {
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1',
    kind: raw.kind || 'ResourceName',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
    },
    spec: {
      // recursively normalize spec fields
    },
  }
}

/**
 * Object → YAML string
 */
export function resourceToYaml(resource: ResourceType): string {
  // clean empty fields before serialization
  const clean = removeEmpty(resource)
  return yaml.dump(clean, { lineWidth: -1, noRefs: true })
}

/**
 * YAML string → object
 */
export function yamlToResource(yamlStr: string): ResourceType {
  const raw = yaml.load(yamlStr) as any
  return normalizeResource(raw)
}
```

### Helper Functions

- `removeEmpty(obj)` — recursively remove null, undefined, empty arrays, and empty objects
- Count / statistics functions (as needed, e.g., `countPluginsByStage` for EdgionPlugins)

### Key Points

1. **createEmpty** returns a complete structure with default values for all required fields
2. **normalize** handles edge cases from backend data (missing fields, null values)
3. **toYaml** cleans empty fields first to avoid outputting `field: null` or `field: []`
4. **fromYaml** uses `yaml.load` + normalize as a double guarantee
5. `lineWidth: -1` prevents YAML long-line wrapping
6. `noRefs: true` prevents YAML anchor references
