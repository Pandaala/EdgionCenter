import type { ResourceKind } from '@/api/types'

export type ResourceAction = 'refresh' | 'search' | 'age' | 'namespace' | 'create' | 'row-view' | 'row-edit' | 'row-delete' | 'select' | 'select-all' | 'batch-delete' | 'row-replace' | 'tab'

export function resourceActionTestId(kind: ResourceKind, action: ResourceAction): string {
  return `${kind}-${action}`
}
