import { describe, expect, it } from 'vitest'
import { RESOURCE_CATALOG, getResourceCatalogEntry, listFirstClassResources } from './resourceCatalog'

describe('resource catalog', () => {
  it('accounts for all 21 Controller resource kinds', () => {
    expect(RESOURCE_CATALOG.size).toBe(21)
    expect(listFirstClassResources()).toHaveLength(19)
  })

  it('keeps Secret and ConfigMap as restricted dependencies', () => {
    expect(getResourceCatalogEntry('secret').lifecycle).toBe('restrictedDependency')
    expect(getResourceCatalogEntry('configmap').lifecycle).toBe('restrictedDependency')
  })

  it('registers EdgionBackendTrafficPolicy as a first-class namespaced resource', () => {
    expect(getResourceCatalogEntry('edgionbackendtrafficpolicy')).toMatchObject({
      scope: 'namespaced',
      lifecycle: 'firstClass',
      route: 'services/backend-traffic-policies',
      hasConditions: true,
    })
  })

  it('declares non-spec Kubernetes operator top-level fields explicitly', () => {
    expect(getResourceCatalogEntry('endpointslice').operatorTopLevelFields).toEqual([
      'addressType', 'endpoints', 'ports',
    ])
    expect(getResourceCatalogEntry('configmap').operatorTopLevelFields).toEqual([
      'data', 'binaryData', 'immutable',
    ])
    expect(getResourceCatalogEntry('secret').operatorTopLevelFields).toEqual([
      'data', 'stringData', 'type', 'immutable',
    ])
    expect(getResourceCatalogEntry('service').operatorTopLevelFields).toEqual(['spec'])
  })

  it('has a complete mutation boundary for every resource', () => {
    for (const entry of RESOURCE_CATALOG.values()) {
      expect(entry.operatorTopLevelFields.length).toBeGreaterThan(0)
      expect(Array.isArray(entry.excludedMutationPaths)).toBe(true)
    }
  })

  it('declares only the Controller-supported alternate API versions', () => {
    expect(getResourceCatalogEntry('referencegrant').acceptedApiVersions).toEqual([
      'gateway.networking.k8s.io/v1beta1',
    ])
    expect(getResourceCatalogEntry('tlsroute').acceptedApiVersions).toEqual([
      'gateway.networking.k8s.io/v1alpha3',
    ])
    expect(getResourceCatalogEntry('backendtlspolicy').acceptedApiVersions).toEqual([
      'gateway.networking.k8s.io/v1alpha3',
    ])
    expect(getResourceCatalogEntry('httproute').acceptedApiVersions).toBeUndefined()
  })
})
