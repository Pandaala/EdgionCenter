import { describe, expect, it } from 'vitest'
import {
  CONTROLLER_ACCESS_RESOURCE_KINDS,
  CONTROLLER_KIND_BY_RESOURCE_KIND,
  controllerAccessPath,
  parseControllerAccessDocument,
} from './access'
import { CONTROLLER_ACCESS_OPERATIONS } from './types'
import { getResourceCatalogEntry } from '@/config/resourceCatalog'

function validDocument() {
  return {
    schemaVersion: 1,
    revision: `sha256:${'0'.repeat(64)}`,
    resources: CONTROLLER_ACCESS_RESOURCE_KINDS.map((kind) => ({
      kind: CONTROLLER_KIND_BY_RESOURCE_KIND[kind],
      scope: getResourceCatalogEntry(kind).scope,
      verbs: kind === 'httproute' ? ['get', 'list', 'create', 'update'] : [],
    })),
    operations: CONTROLLER_ACCESS_OPERATIONS.map((name) => ({ name, allowed: name === 'serverInfo' })),
  }
}

describe('Controller access contract', () => {
  it('accepts the complete bounded v1 document', () => {
    expect(parseControllerAccessDocument(validDocument())).toMatchObject({
      schemaVersion: 1,
      resources: expect.arrayContaining([
        expect.objectContaining({ kind: 'HTTPRoute', verbs: ['get', 'list', 'create', 'update'] }),
      ]),
    })
  })

  it.each([
    ['old schema', (doc: ReturnType<typeof validDocument>) => { doc.schemaVersion = 0 }],
    ['invalid revision', (doc: ReturnType<typeof validDocument>) => { doc.revision = '1' }],
    ['missing resource', (doc: ReturnType<typeof validDocument>) => { doc.resources.pop() }],
    ['duplicate/out-of-order resource', (doc: ReturnType<typeof validDocument>) => { doc.resources[1] = doc.resources[0] }],
    ['unknown verb', (doc: ReturnType<typeof validDocument>) => { doc.resources[0].verbs = ['patch'] }],
    ['out-of-order verbs', (doc: ReturnType<typeof validDocument>) => { doc.resources[0].verbs = ['update', 'get'] }],
    ['missing operation', (doc: ReturnType<typeof validDocument>) => { doc.operations.pop() }],
    ['out-of-order operation', (doc: ReturnType<typeof validDocument>) => { doc.operations.reverse() }],
  ])('rejects an incompatible document: %s', (_name, mutate) => {
    const doc = validDocument()
    mutate(doc)
    expect(() => parseControllerAccessDocument(doc)).toThrow()
  })

  it('uses direct and explicitly selected-Controller paths', () => {
    expect(controllerAccessPath(null)).toBe('/api/v1/access')
    expect(controllerAccessPath('east/controller-1')).toBe(
      '/api/v1/proxy/east~controller-1/api/v1/access',
    )
  })
})
