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

## Lossless Resource Adapter Pattern

File location: `src/utils/{resource}.ts`

Reference:
- `src/utils/httproute.ts` (190 lines)
- `src/utils/edgionplugins.ts` (129 lines)

Resource adapters must distinguish the API view from the operator document sent
on mutation. Do not project a backend response onto the fields currently rendered
by the form.

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
export function parseResource(raw: unknown): ResourceType {
  // Validate identity and required shape, but retain every operator-owned field.
  return validateWithoutProjection(raw)
}

export function toEditableDocument(view: ResourceType): ResourceType {
  // Strip status, server-owned metadata, and documented runtime/internal paths.
  return projectOperatorFields(view)
}

export function toMutationDocument(editable: ResourceType, mode: 'create' | 'update') {
  // Update may retain resourceVersion for optimistic concurrency. Never retain
  // parsed/resolved/redacted values or other server-owned fields.
  return buildMutationEnvelope(editable, mode)
}

/**
 * Object → YAML string
 */
export function resourceToYaml(resource: ResourceType): string {
  return yaml.dump(resource, { lineWidth: -1, noRefs: true })
}

/**
 * YAML string → object
 */
export function yamlToResource(yamlStr: string): ResourceType {
  return parseResource(yaml.load(yamlStr))
}
```

### Data channels

- **Operator-owned**: editable metadata and non-internal spec fields.
- **Server-owned**: status, uid, generation, managedFields, creationTimestamp.
- **Runtime/internal**: `schemars(skip)`, parsed, compiled, resolved, denial,
  and redacted paths.

Adapters preserve unknown operator spec fields for forward compatibility while
explicitly stripping known internal paths.

### Key Points

1. **createEmpty** supplies defaults only for a newly created resource.
2. Parsing an existing resource must not inject defaults or collapse arrays.
3. Structured form changes use narrow immutable patches and preserve siblings.
4. Generic `removeEmpty` is forbidden on an existing operator document. Empty
   and absent are equivalent only when the authoritative contract says so.
5. Every adapter needs current, unknown-field, multi-entry, and internal-strip
   preservation fixtures.
6. `lineWidth: -1` prevents YAML long-line wrapping.
7. `noRefs: true` prevents YAML anchor references.
