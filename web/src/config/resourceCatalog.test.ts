import { describe, expect, it } from 'vitest'
import { buildMutationDocument } from '@/utils/resource-document'
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

  it.each([
    'requestPlugins',
    'upstreamResponseFilterPlugins',
    'upstreamResponseBodyFilterPlugins',
    'upstreamResponsePlugins',
  ])('removes entry-level policyAction from EdgionPlugins %s without projecting operator fields', (stage) => {
    const mutation = buildMutationDocument({
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionPlugins',
      metadata: { name: 'plugins', namespace: 'edge' },
      spec: {
        [stage]: [{
          type: 'Example',
          alias: `${stage}-alias`,
          enable: false,
          policyAction: 'runtime-only',
          config: { futureConfig: true, policyAction: 'config-owned' },
        }],
      },
    }, { resourceKind: 'edgionplugins', mode: 'update' })

    expect(mutation).not.toHaveProperty(`spec.${stage}.0.policyAction`)
    expect(mutation).toHaveProperty(`spec.${stage}.0.alias`, `${stage}-alias`)
    expect(mutation).toHaveProperty(`spec.${stage}.0.enable`, false)
    expect(mutation).toHaveProperty(`spec.${stage}.0.config`, {
      futureConfig: true,
      policyAction: 'config-owned',
    })
  })

  it.each([
    {
      resourceKind: 'edgiongatewayconfig' as const,
      apiVersion: 'edgion.io/v1alpha1',
      kind: 'EdgionGatewayConfig',
      spec: { enableReferenceGrantValidation: true, futureSpec: true },
      removed: ['spec.enableReferenceGrantValidation'],
    },
    {
      resourceKind: 'edgionacme' as const,
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionAcme',
      spec: { renewal: { renewBeforeDays: 30, futureRenewal: true }, futureSpec: true },
      removed: ['spec.renewal.renewBeforeDays'],
    },
    {
      resourceKind: 'edgionbackendtrafficpolicy' as const,
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionBackendTrafficPolicy',
      spec: {
        outlierDetection: { ejectionSeconds: 30, maxEjectionSeconds: 300, futureOutlier: true },
        futureSpec: true,
      },
      removed: ['spec.outlierDetection.ejectionSeconds', 'spec.outlierDetection.maxEjectionSeconds'],
    },
  ])('strips known obsolete $kind fields without projecting siblings', ({ resourceKind, apiVersion, kind, spec, removed }) => {
    const mutation = buildMutationDocument({
      apiVersion,
      kind,
      metadata: { name: 'resource', namespace: 'edge' },
      spec,
    }, { resourceKind, mode: 'update' })

    for (const path of removed) expect(mutation).not.toHaveProperty(path)
    expect(mutation).toHaveProperty('spec.futureSpec', true)
  })

  it('strips removed ForwardAuth degradation fields from plugin mutations', () => {
    const mutation = buildMutationDocument({
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionPlugins',
      metadata: { name: 'plugins', namespace: 'edge' },
      spec: {
        requestPlugins: [{
          type: 'ForwardAuth',
          config: {
            allowDegradation: true,
            allowDegradationTemplate: '${ctx:degrade}',
            decision: { futureDecision: true },
            futureConfig: true,
          },
        }],
      },
    }, { resourceKind: 'edgionplugins', mode: 'update' })

    expect(mutation).not.toHaveProperty('spec.requestPlugins.0.config.allowDegradation')
    expect(mutation).not.toHaveProperty('spec.requestPlugins.0.config.allowDegradationTemplate')
    expect(mutation).toHaveProperty('spec.requestPlugins.0.config.decision.futureDecision', true)
    expect(mutation).toHaveProperty('spec.requestPlugins.0.config.futureConfig', true)
  })

  it.each([
    'plugins',
    'tlsRoutePlugins',
  ])('removes entry-level policyAction from EdgionStreamPlugins %s without projecting operator fields', (stage) => {
    const mutation = buildMutationDocument({
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionStreamPlugins',
      metadata: { name: 'stream-plugins', namespace: 'edge' },
      spec: {
        [stage]: [{
          type: 'Example',
          enable: true,
          policyAction: 'runtime-only',
          config: { futureConfig: [], policyAction: 'config-owned' },
        }],
      },
    }, { resourceKind: 'edgionstreamplugins', mode: 'update' })

    expect(mutation).not.toHaveProperty(`spec.${stage}.0.policyAction`)
    expect(mutation).toHaveProperty(`spec.${stage}.0.type`, 'Example')
    expect(mutation).toHaveProperty(`spec.${stage}.0.enable`, true)
    expect(mutation).toHaveProperty(`spec.${stage}.0.config`, {
      futureConfig: [],
      policyAction: 'config-owned',
    })
  })
})
