import { describe, expect, it } from 'vitest'
import { buildMutationDocument } from '@/utils/resource-document'
import { RESOURCE_CATALOG, getResourceCatalogEntry, listFirstClassResources } from './resourceCatalog'

describe('resource catalog', () => {
  it.each(['requestPlugins', 'upstreamResponseFilterPlugins', 'upstreamResponseBodyFilterPlugins', 'upstreamResponsePlugins'])(
    'removes resolved dye conditions in %s while retaining authored rules', (stage) => {
      const rule = { name: 'x-tag', on: ['success'], value: 'authored', conditions: { run: { allOf: [{
        type: 'secretMatch', key: { type: 'header', name: 'x-key' }, secretRef: { name: 'keys' }, secretKey: 'values',
        resolvedValues: ['private-runtime-value'],
      }] } } }
      const source = {
        apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins', metadata: { name: 'dye', namespace: 'edge' },
        spec: { [stage]: [{ type: 'Mock', config: {}, dye: { request: [structuredClone(rule)], response: [structuredClone(rule)] } }] },
      }
      const before = structuredClone(source)
      for (const mode of ['create', 'update'] as const) {
        const mutation: any = buildMutationDocument(source, { resourceKind: 'edgionplugins', mode })
        for (const direction of ['request', 'response']) {
          const output = mutation.spec[stage][0].dye[direction][0]
          expect(output.name).toBe('x-tag')
          expect(output.value).toBe('authored')
          expect(output.conditions.run.allOf[0]).toEqual({
            type: 'secretMatch', key: { type: 'header', name: 'x-key' }, secretRef: { name: 'keys' }, secretKey: 'values',
          })
        }
      }
      expect(source).toEqual(before)
    },
  )

  it.each(['create', 'update'] as const)('preserves authored plugin maps and JSON literals on %s', (mode) => {
    const literal = { resolvedSecrets: 'operator data', nested: { refDenied: false }, compiledRegex: null }
    const source = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins', metadata: { name: 'opaque', namespace: 'edge' },
      spec: { requestPlugins: [
        { type: 'Mock', config: { headers: { resolvedSecrets: 'response-header' } } },
        { type: 'Wasm', config: {
          pluginConfig: { json: structuredClone(literal) }, vmConfig: { json: structuredClone(literal) },
          source: { url: 'https://modules.example.com/plugin.wasm', fetch: {
            authHeaderSecretRef: { name: 'pull', key: 'token' }, resolvedAuthHeader: '[redacted]',
            tls: { enabled: true, clientCertificateRef: { name: 'identity' }, resolvedClientCertificate: '[redacted]' },
          } },
        } },
        { type: 'ProxyRewrite', config: { jsonBody: { operations: [{ op: 'add', path: '/data', value: structuredClone(literal) }] } } },
        { type: 'Canary', config: { enable: true, activeProfile: 'resolvedSecrets', profiles: { resolvedSecrets: { name: 'api' } }, activeProfileRef: { name: 'selector', refDenied: {} } } },
      ] },
    }
    const before = structuredClone(source)
    const mutation: any = buildMutationDocument(source, { resourceKind: 'edgionplugins', mode })
    expect(mutation.spec.requestPlugins[0].config.headers).toEqual({ resolvedSecrets: 'response-header' })
    expect(mutation.spec.requestPlugins[1].config.pluginConfig.json).toEqual(literal)
    expect(mutation.spec.requestPlugins[1].config.vmConfig.json).toEqual(literal)
    expect(mutation.spec.requestPlugins[1].config.source.fetch).toEqual({
      authHeaderSecretRef: { name: 'pull', key: 'token' },
      tls: { enabled: true, clientCertificateRef: { name: 'identity' } },
    })
    expect(mutation.spec.requestPlugins[2].config.jsonBody.operations[0].value).toEqual(literal)
    expect(mutation.spec.requestPlugins[3].config).toEqual({
      enable: true, activeProfile: 'resolvedSecrets', profiles: { resolvedSecrets: { name: 'api' } }, activeProfileRef: { name: 'selector' },
    })
    expect(source).toEqual(before)
  })

  it.each(['create', 'update'] as const)('strips deployment runtime status in %s while retaining operator siblings', (mode) => {
    const kinds = [
      'gatewayclass', 'edgiongatewayconfig', 'gateway', 'httproute', 'grpcroute',
      'tcproute', 'udproute', 'tlsroute', 'edgiontls', 'backendtlspolicy',
      'edgionplugins', 'edgionstreamplugins', 'edgionconfigdata', 'edgionacme',
      'linksys', 'edgionbackendtrafficpolicy',
    ] as const
    for (const kind of kinds) {
      const entry = getResourceCatalogEntry(kind)
      const source = {
        apiVersion: entry.apiVersion, kind: entry.displayName,
        metadata: { name: 'example', namespace: 'edge', resourceVersion: '7' },
        status: { controllers: [{ controllerName: 'east', status: { conditions: [] } }] },
        spec: { currentStatus: { conditions: [] }, futureField: { currentStatus: 'operator-value' } },
      }
      expect(buildMutationDocument(source, { resourceKind: kind, mode })).toEqual({
        apiVersion: entry.apiVersion, kind: entry.displayName,
        metadata: { name: 'example', namespace: 'edge', ...(mode === 'update' ? { resourceVersion: '7' } : {}) },
        spec: { futureField: { currentStatus: 'operator-value' } },
      })
      expect(source.spec.currentStatus).toEqual({ conditions: [] })
    }
  })

  it('omits Controller attachment proofs and status identity while retaining operator siblings', () => {
    for (const kind of ['httproute', 'grpcroute', 'tcproute', 'udproute', 'tlsroute', 'edgiontls'] as const) {
      const entry = getResourceCatalogEntry(kind)
      const internal = kind === 'edgiontls' ? { resolvedListenerAttachments: [{ gatewayProof: { epoch: 'runtime' } }] }
        : { resolvedStatusController: 'controller', ...(['tcproute', 'udproute'].includes(kind) ? { resolvedListenerAttachments: [] } : {}) }
      const resource = { apiVersion: entry.apiVersion, kind: entry.displayName, metadata: { name: 'route', namespace: 'edge' }, spec: { ...internal, future: { resolvedStatusController: 'operator' } } }
      expect(buildMutationDocument(resource, { resourceKind: kind, mode: 'update' }).spec).toEqual({ future: { resolvedStatusController: 'operator' } })
    }
  })

  it('accounts for all 22 Controller resource kinds', () => {
    expect(RESOURCE_CATALOG.size).toBe(22)
    expect(listFirstClassResources()).toHaveLength(20)
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
    for (const kind of ['tcproute', 'udproute'] as const) {
      expect(getResourceCatalogEntry(kind).apiVersion).toBe('gateway.networking.k8s.io/v1')
      expect(getResourceCatalogEntry(kind).acceptedApiVersions).toEqual(['gateway.networking.k8s.io/v1alpha2'])
    }
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
