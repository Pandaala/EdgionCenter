import { describe, expect, it } from 'vitest'
import { getResourceCatalogEntry } from './resourceCatalog'

describe('restricted dependency operation policy',()=>{
  it.each(['secret','configmap'] as const)('%s permits metadata/create/replace but not delete or value view',(kind)=>{
    const entry=getResourceCatalogEntry(kind)
    expect(entry.lifecycle).toBe('restrictedDependency')
    expect(entry.restrictedOperations).toEqual(['list-keys','create','update'])
    expect(entry.restrictedOperations).not.toContain('delete')
  })
})
