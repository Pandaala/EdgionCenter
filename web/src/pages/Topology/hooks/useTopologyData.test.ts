import { describe, expect, it } from 'vitest'
import type { K8sResource } from '@/api/types'
import { buildTopologyGraph, TOPOLOGY_KINDS } from './useTopologyData'
import { TOPOLOGY_EDGE_COLORS } from '../components/TopologyCanvas'

function resource(kind: string, name: string, namespace: string | undefined, spec: unknown = {}, status?: unknown): K8sResource {
  return { apiVersion: 'test/v1', kind, metadata: { name, namespace }, spec, status }
}

describe('buildTopologyGraph', () => {
  it('links WAF policy, selector and bundles using each owning resource namespace', () => {
    const graph = buildTopologyGraph({
      edgionplugins: [resource('EdgionPlugins', 'waf', 'edge', {
        waf: { policyRef: { name: 'policy', namespace: 'security' }, activeProfileRef: { name: 'selector' } },
        future: { policyRef: { name: 'unrelated' } },
      })],
      edgionconfigdata: [
        resource('EdgionConfigData', 'policy', 'security', { data: { type: 'WafPolicy', config: {
          profiles: { base: { bundleRefs: [{ name: 'rules' }, { name: 'extra', namespace: 'shared', optional: true }] } },
        } } }),
        resource('EdgionConfigData', 'selector', 'edge'),
        resource('EdgionConfigData', 'rules', 'security'),
        resource('EdgionConfigData', 'misc', 'security', { data: { type: 'Misc', config: {
          profiles: { base: { bundleRefs: [{ name: 'unrelated' }] } },
        } } }),
      ],
    }, null, new Set(), true)
    const edges = graph.edges.map((edge) => `${edge.source}->${edge.target}`)
    expect(edges).toContain('edgionplugins/edge/waf->edgionconfigdata/security/policy')
    expect(edges).toContain('edgionplugins/edge/waf->edgionconfigdata/edge/selector')
    expect(edges).toContain('edgionconfigdata/security/policy->edgionconfigdata/security/rules')
    expect(edges).toContain('edgionconfigdata/security/policy->edgionconfigdata/shared/extra')
    expect(graph.nodes.some((node) => node.data.name === 'unrelated')).toBe(false)
  })
  it('links Gateway operator TLS references without scanning private certificate projections', () => {
    const graph = buildTopologyGraph({
      gateway: [resource('Gateway', 'edge', 'demo', {
        tls: {
          backend: { clientCertificateRef: { name: 'client', namespace: 'pki' }, resolvedClientCertificate: { secretRef: { name: 'private' } } },
          frontend: { default: { validation: { caCertificateRefs: [{ group: '', kind: 'ConfigMap', name: 'default-ca' }] } },
            perPort: [{ port: 443, tls: { validation: { caCertificateRefs: [{ group: '', kind: 'Secret', name: 'port-ca', namespace: 'pki' }] } } }] },
        },
        listeners: [{ name: 'https', tls: { frontendValidation: { caCertificateRefs: [{ group: '', kind: 'Secret', name: 'private' }] } } }],
      })],
      secret: [resource('Secret', 'client', 'pki'), resource('Secret', 'port-ca', 'pki')],
      configmap: [resource('ConfigMap', 'default-ca', 'demo')],
      referencegrant: [resource('ReferenceGrant', 'tls', 'pki', {
        from: [{ group: 'gateway.networking.k8s.io', kind: 'Gateway', namespace: 'demo' }],
        to: [{ group: '', kind: 'Secret' }],
      })],
    }, 'demo', new Set(), true)
    for (const [target, label] of [
      ['secret/pki/client', 'backend client certificate'],
      ['configmap/demo/default-ca', 'frontend CA (default)'],
      ['secret/pki/port-ca', 'frontend CA (port 443)'],
    ]) expect(graph.edges).toContainEqual(expect.objectContaining({ source: 'gateway/demo/edge', target, label, state: 'resolved' }))
    expect(graph.nodes.some(node => node.data.name === 'private')).toBe(false)
    expect(graph.edges).toContainEqual(expect.objectContaining({ source: 'gateway/demo/edge', target: 'referencegrant/pki/tls', label: 'granted' }))
  })

  it('distinguishes unavailable CA inventory from a missing CA', () => {
    const gateway = resource('Gateway', 'edge', 'demo', { tls: { frontend: { default: { validation: {
      caCertificateRefs: [{ group: '', kind: 'ConfigMap', name: 'ca' }],
    } } } } })
    const unavailable = buildTopologyGraph({ gateway: [gateway] }, null, new Set(['configmap']))
    expect(unavailable.edges[0]).toMatchObject({ target: 'configmap/demo/ca', state: 'unavailable' })
    const missing = buildTopologyGraph({ gateway: [gateway] }, null)
    expect(missing.edges[0]).toMatchObject({ target: 'configmap/demo/ca', state: 'unresolved' })
  })

  it.each([
    { kind: 'Secret', name: 'ca' },
    { group: 'core', kind: 'Secret', name: 'ca' },
    { group: '', name: 'ca' },
    { group: '', kind: 'Service', name: 'ca' },
    { group: 'foreign.example', kind: 'ConfigMap', name: 'ca' },
  ])('keeps malformed frontend CA references unknown: %j', reference => {
    const graph = buildTopologyGraph({ gateway: [resource('Gateway', 'edge', 'demo', { tls: { frontend: {
      default: { validation: { caCertificateRefs: [reference] } },
    } } })] }, null)
    expect(graph.edges[0]).toMatchObject({ target: 'unknown/demo/ca', state: 'unknown' })
  })

  it('builds gateway-to-backend and policy/dependency relationships', () => {
    const graph = buildTopologyGraph({
      gatewayclass: [resource('GatewayClass', 'edgion', undefined)],
      gateway: [resource('Gateway', 'edge', 'demo', { gatewayClassName: 'edgion' })],
      httproute: [resource('HTTPRoute', 'web', 'demo', {
        parentRefs: [{ name: 'edge' }],
        rules: [{
          backendRefs: [{ name: 'web-svc', port: 8080 }],
          filters: [{ type: 'ExtensionRef', extensionRef: { group: 'edgion.io', kind: 'EdgionPlugins', name: 'auth' } }],
        }],
      })],
      service: [resource('Service', 'web-svc', 'demo')],
      endpointslice: [{
        ...resource('EndpointSlice', 'web-svc-a', 'demo'),
        metadata: { name: 'web-svc-a', namespace: 'demo', labels: { 'kubernetes.io/service-name': 'web-svc' } },
        endpoints: [{ addresses: ['10.0.0.8'], conditions: { ready: true } }],
      } as K8sResource],
      edgionplugins: [resource('EdgionPlugins', 'auth', 'demo', {
        requestPlugins: [{ type: 'KeyAuth', config: { secretRefs: [{ name: 'api-keys' }] } }],
      })],
      secret: [resource('Secret', 'api-keys', 'demo')],
      backendtlspolicy: [resource('BackendTLSPolicy', 'web-tls', 'demo', { targetRefs: [{ kind: 'Service', name: 'web-svc' }] })],
      edgionacme: [resource('EdgionAcme', 'cert', 'demo', {
        privateKeySecretRef: { name: 'acme-account' }, storage: { secretName: 'web-cert' },
        autoEdgionTls: { enabled: true, name: 'web-tls' },
      })],
      edgiontls: [resource('EdgionTls', 'web-tls', 'demo', { secretRef: { name: 'web-cert' } })],
    }, null, new Set(), true)

    const edgePairs = graph.edges.map((edge) => `${edge.source}->${edge.target}`)
    expect(edgePairs).toContain('gatewayclass/_cluster/edgion->gateway/demo/edge')
    expect(edgePairs).toContain('gateway/demo/edge->httproute/demo/web')
    expect(edgePairs).toContain('httproute/demo/web->service/demo/web-svc')
    expect(edgePairs).toContain('httproute/demo/web->edgionplugins/demo/auth')
    expect(edgePairs).toContain('edgionplugins/demo/auth->secret/demo/api-keys')
    expect(edgePairs).toContain('service/demo/web-svc->backendtlspolicy/demo/web-tls')
    expect(edgePairs).toContain('service/demo/web-svc->endpointslice/demo/web-svc-a')
    expect(edgePairs).toContain('edgionacme/demo/cert->edgiontls/demo/web-tls')
    expect(edgePairs).toContain('edgionacme/demo/cert->secret/demo/web-cert')
    expect(graph.nodes.some((node) => node.data.kind === 'backend' && node.data.name === '10.0.0.8')).toBe(true)
  })

  it('surfaces unresolved references and condition conflicts', () => {
    const graph = buildTopologyGraph({
      httproute: [resource('HTTPRoute', 'broken', 'demo', {
        rules: [{ backendRefs: [{ name: 'missing' }] }],
      }, {
        parents: [{ parentRef: { name: 'edge' }, conditions: [{ type: 'Conflicted', status: 'True', reason: 'ListenerConflict', message: 'listener conflict' }] }],
      })],
    }, null, new Set(), true)
    expect(graph.nodes.find((node) => node.id === 'service/demo/missing')?.data.unresolved).toBe(true)
    expect(graph.nodes.find((node) => node.id === 'httproute/demo/broken')?.data.conflict).toBe(true)
    expect(graph.edges.find((edge) => edge.target === 'service/demo/missing')?.state).toBe('unresolved')
  })

  it('does not misclassify NoConflicts=False and distinguishes unavailable kinds', () => {
    const graph = buildTopologyGraph({
      httproute: [resource('HTTPRoute', 'route', 'demo', { rules: [{ backendRefs: [{ name: 'svc' }] }] }, {
        conditions: [{ type: 'NoConflicts', status: 'False', reason: 'ConflictsFound' }],
      })],
    }, null, new Set(['service']))
    expect(graph.nodes.find((node) => node.id === 'httproute/demo/route')?.data.conflict).toBe(false)
    const service = graph.nodes.find((node) => node.id === 'service/demo/svc')
    expect(service?.data.unavailable).toBe(true)
    expect(service?.data.unresolved).toBe(false)
    expect(graph.edges.find((edge) => edge.target === 'service/demo/svc')?.state).toBe('unavailable')
    expect(TOPOLOGY_EDGE_COLORS.unavailable).not.toBe(TOPOLOGY_EDGE_COLORS.unresolved)
  })

  it('keeps explicit unknown groups unknown and builds current annotation/policy edges', () => {
    const graph = buildTopologyGraph({
      edgiongatewayconfig: [resource('EdgionGatewayConfig', 'global', undefined, { globalPluginsRef: [{ name: 'global-p' }] })],
      gateway: [resource('Gateway', 'edge', 'demo', { listeners: [{ name: 'https', tls: { certificateRefs: [{ name: 'cert' }] } }] })],
      tcproute: [{ ...resource('TCPRoute', 'tcp', 'demo', { rules: [{ backendRefs: [{ group: 'evil.io', kind: 'Service', name: 'svc' }] }] }), metadata: { name: 'tcp', namespace: 'demo', annotations: { 'edgion.io/edgion-stream-plugins': 'stream-p' } } }],
      backendtlspolicy: [resource('BackendTLSPolicy', 'btp', 'demo', { targetRefs: [{ kind: 'Service', name: 'svc' }], options: { 'edgion.io/client-certificate-ref': 'client-cert' } })],
      edgionplugins: [resource('EdgionPlugins', 'wasm', 'demo', { requestPlugins: [{ type: 'Wasm', config: { source: { url: 'https://modules/x.wasm', fetch: { authHeaderSecretRef: { secret: { name: 'wasm-auth' }, key: 'authorization' } } } } }] })],
      referencegrant: [resource('ReferenceGrant', 'allow', 'target', { from: [{ group: 'gateway.networking.k8s.io', kind: 'HTTPRoute', namespace: 'source' }], to: [{ group: '', kind: 'Secret', name: 'cert' }] })],
    }, null)
    const pairs = graph.edges.map((edge) => `${edge.source}->${edge.target}:${edge.label}`)
    expect(pairs).toContain('edgiongatewayconfig/_cluster/global->edgionplugins/default/global-p:global plugin')
    expect(pairs).toContain('gateway/demo/edge->secret/demo/cert:certificate#https')
    expect(pairs).toContain('tcproute/demo/tcp->edgionstreamplugins/demo/stream-p:stream plugin annotation')
    expect(pairs).toContain('backendtlspolicy/demo/btp->secret/demo/client-cert:client certificate')
    expect(pairs).toContain('edgionplugins/demo/wasm->secret/demo/wasm-auth:authHeaderSecretRef')
    expect(graph.nodes.some((node) => node.data.kind === 'unknown' && node.data.name === 'svc')).toBe(true)
    expect(graph.edges.find((edge) => edge.target.includes('unknown/') && edge.target.endsWith('/svc'))?.state).toBe('unknown')
    expect(graph.nodes.some((node) => node.data.kind === 'referencegrant')).toBe(true)
  })

  it('resolves client certificate whitespace in the policy namespace', () => {
    const graph = buildTopologyGraph({
      backendtlspolicy: [resource('BackendTLSPolicy', 'btp', 'demo', {
        options: { 'edgion.io/client-certificate-ref': '  client-cert\n' },
      })],
      secret: [resource('Secret', 'client-cert', 'demo')],
    }, null)
    expect(graph.edges).toContainEqual(expect.objectContaining({
      source: 'backendtlspolicy/demo/btp', target: 'secret/demo/client-cert',
      label: 'client certificate', state: 'resolved',
    }))
    expect(graph.nodes.filter(node => node.data.kind === 'secret')).toHaveLength(1)
  })

  it.each(['other/client-cert', 'client..cert', 'Upper', 'a'.repeat(64)])(
    'keeps invalid client certificate %j unknown without a cross-namespace grant', value => {
      const graph = buildTopologyGraph({
        backendtlspolicy: [resource('BackendTLSPolicy', 'btp', 'demo', {
          options: { 'edgion.io/client-certificate-ref': value },
        })],
        secret: [resource('Secret', 'client-cert', 'other'), resource('Secret', value, 'demo')],
      }, null, new Set(), true)
      expect(graph.edges).toHaveLength(1)
      expect(graph.edges[0]).toMatchObject({
        source: 'backendtlspolicy/demo/btp', target: `unknown/demo/${value}`,
        label: 'invalid client certificate', state: 'unknown',
      })
      expect(graph.nodes.some(node => node.data.kind === 'referencegrant')).toBe(false)
    },
  )

  it.each([undefined, '', '   ', null, { name: 'client-cert', namespace: 'other' }])(
    'does not invent a Secret dependency from empty or non-string option %j', value => {
      const graph = buildTopologyGraph({
        backendtlspolicy: [resource('BackendTLSPolicy', 'btp', 'demo', {
          options: { 'edgion.io/client-certificate-ref': value },
        })],
      }, null)
      expect(graph.edges).toEqual([])
    },
  )

  it('projects matching and missing ReferenceGrant decisions for cross-namespace refs', () => {
    const graph = buildTopologyGraph({
      httproute: [
        resource('HTTPRoute', 'allowed', 'source', { rules: [{ backendRefs: [{ name: 'svc', namespace: 'target' }] }] }),
        resource('HTTPRoute', 'denied', 'other', { rules: [{ backendRefs: [{ name: 'svc', namespace: 'target' }] }] }),
      ],
      service: [resource('Service', 'svc', 'target')],
      referencegrant: [resource('ReferenceGrant', 'routes', 'target', {
        from: [{ group: 'gateway.networking.k8s.io', kind: 'HTTPRoute', namespace: 'source' }],
        to: [{ group: '', kind: 'Service', name: 'svc' }],
      })],
    }, null, new Set(), true)
    expect(graph.edges.some((edge) => edge.source === 'httproute/source/allowed' && edge.target === 'referencegrant/target/routes' && edge.label === 'granted')).toBe(true)
    expect(graph.nodes.some((node) => node.id.includes('referencegrant/target/denied:Service/svc') && node.data.rejected)).toBe(true)
  })
  it('does not synthesize denial while ReferenceGrant validation is disabled or unknown', () => {
    const resources = {
      httproute: [resource('HTTPRoute', 'cross', 'source', { rules: [{ backendRefs: [{ name: 'svc', namespace: 'target' }] }] })],
      service: [resource('Service', 'svc', 'target')],
    }
    const disabled = buildTopologyGraph(resources, null, new Set(), false)
    expect(disabled.nodes.some((node) => node.data.name.startsWith('denied:'))).toBe(false)
    expect(disabled.nodes.some((node) => node.data.name.startsWith('grant-check-unavailable:'))).toBe(false)
    const unknown = buildTopologyGraph(resources, null, new Set(), 'unknown')
    expect(unknown.nodes.some((node) => node.data.name.startsWith('denied:'))).toBe(false)
    expect(unknown.nodes.some((node) => node.data.name.startsWith('grant-check-unavailable:') && node.data.unavailable)).toBe(true)
    expect(unknown.edges.some((edge) => edge.state === 'unknown' && edge.label === 'grant check unavailable')).toBe(true)
  })

  it('keeps connected cluster parents when filtering a namespace', () => {
    const graph = buildTopologyGraph({
      gatewayclass: [resource('GatewayClass', 'edgion', undefined)],
      gateway: [resource('Gateway', 'edge', 'demo', { gatewayClassName: 'edgion' })],
      service: [resource('Service', 'other', 'other')],
    }, 'demo')
    expect(graph.nodes.map((node) => node.id)).toEqual(expect.arrayContaining([
      'gateway/demo/edge', 'gatewayclass/_cluster/edgion',
    ]))
    expect(graph.nodes.map((node) => node.id)).not.toContain('service/other/other')
  })
})


it('retains references for a partially invalid route and separates stale conflict', () => {
  const value = resource('HTTPRoute', 'web', 'demo', {
    rules: [{ backendRefs: [{ name: 'svc' }, { name: 'missing' }] }],
  }, { parents: [{ parentRef: { name: 'edge' }, conditions: [
    { type: 'Conflicted', status: 'True', observedGeneration: 1 },
    { type: 'PartiallyInvalid', status: 'True', observedGeneration: 2 },
    { type: 'ResolvedRefs', status: 'False', observedGeneration: 2 },
  ] }] })
  value.metadata.generation = 2
  const graph = buildTopologyGraph({
    httproute: [value], service: [resource('Service', 'svc', 'demo')],
  }, null, new Set(), true)
  expect(graph.nodes.find((node) => node.id === 'httproute/demo/web')?.data).toMatchObject({
    conflict: false, partiallyInvalid: true, stale: true, unresolvedConditions: true,
  })
  expect(graph.edges.find((edge) => edge.target === 'service/demo/svc')?.state).toBe('resolved')
  expect(graph.edges.find((edge) => edge.target === 'service/demo/missing')?.state).toBe('unresolved')
})


it('loads AI backends and connects routes, policies, credential Secrets and Redis', () => {
  expect(TOPOLOGY_KINDS).toContain('edgionbackend')
  const graph = buildTopologyGraph({
    httproute: [resource('HTTPRoute', 'ai', 'demo', { rules: [{ backendRefs: [
      { group: 'edgion.io', kind: 'EdgionBackend', name: 'provider' },
      { group: 'other.io', kind: 'EdgionBackend', name: 'foreign' },
    ] }] })],
    edgionbackend: [resource('EdgionBackend', 'provider', 'demo', { ai: {
      credentialPool: { redisRef: 'demo/quota', credentials: [
        { name: 'primary', secretRef: { name: 'key' } },
        { name: 'backup', secretRef: { name: 'absent' } },
      ] },
    } })],
    edgionbackendtrafficpolicy: [resource('EdgionBackendTrafficPolicy', 'retry', 'demo', {
      targetRefs: [{ group: 'edgion.io', kind: 'EdgionBackend', name: 'provider' }],
    })],
    secret: [resource('Secret', 'key', 'demo')],
    linksys: [resource('LinkSys', 'quota', 'demo')],
  }, 'demo')
  expect(graph.nodes.find((node) => node.id === 'edgionbackend/demo/provider')?.data.layer).toBe(2)
  const pairs = graph.edges.map((edge) => `${edge.source}->${edge.target}:${edge.state}`)
  expect(pairs).toEqual(expect.arrayContaining([
    'httproute/demo/ai->edgionbackend/demo/provider:resolved',
    'edgionbackend/demo/provider->edgionbackendtrafficpolicy/demo/retry:resolved',
    'edgionbackend/demo/provider->secret/demo/key:resolved',
    'edgionbackend/demo/provider->secret/demo/absent:unresolved',
    'edgionbackend/demo/provider->linksys/demo/quota:resolved',
    'httproute/demo/ai->unknown/demo/foreign:unknown',
  ]))
})

it('distinguishes unavailable AI backend inventory from a missing backend', () => {
  const resources = { httproute: [resource('HTTPRoute', 'ai', 'demo', { rules: [{ backendRefs: [
    { group: 'edgion.io', kind: 'EdgionBackend', name: 'provider' },
  ] }] })] }
  expect(buildTopologyGraph(resources, null).edges[0].state).toBe('unresolved')
  expect(buildTopologyGraph(resources, null, new Set(['edgionbackend'])).edges[0].state).toBe('unavailable')
})


it.each([true, 'unknown'] as const)('does not apply ReferenceGrant to cross-namespace parent attachment (%s)', (validation) => {
  const graph = buildTopologyGraph({
    httproute: [resource('HTTPRoute', 'web', 'apps', {
      parentRefs: [{ name: 'edge', namespace: 'infra' }],
    })],
    gateway: [resource('Gateway', 'edge', 'infra', { listeners: [
      { name: 'http', port: 80, protocol: 'HTTP', allowedRoutes: { namespaces: { from: 'All' } } },
    ] })],
  }, null, new Set(), validation)
  expect(graph.edges).toHaveLength(1)
  expect(graph.edges[0]).toMatchObject({ source: 'gateway/infra/edge', target: 'httproute/apps/web', label: 'parent' })
  expect(graph.nodes.some((node) => node.data.kind === 'referencegrant')).toBe(false)
})

it('does not claim denial when ReferenceGrant inventory is unavailable', () => {
  const graph = buildTopologyGraph({
    edgionbackend: [resource('EdgionBackend', 'provider', 'apps', { ai: {
      credentialPool: { credentials: [{ name: 'primary', secretRef: { name: 'key', namespace: 'credentials' } }] },
    } })],
    secret: [resource('Secret', 'key', 'credentials')],
  }, null, new Set(['referencegrant']), true)
  expect(graph.nodes.some((node) => node.data.name.startsWith('denied:'))).toBe(false)
  expect(graph.edges.some((edge) => edge.state === 'unknown' && edge.label === 'grant check unavailable')).toBe(true)
})

it('matches an AI credential grant in the credential namespace', () => {
  const graph = buildTopologyGraph({
    edgionbackend: [resource('EdgionBackend', 'provider', 'apps', { ai: {
      credentialPool: { credentials: [{ name: 'primary', secretRef: { name: 'key', namespace: 'credentials' } }] },
    } })],
    secret: [resource('Secret', 'key', 'credentials')],
    referencegrant: [resource('ReferenceGrant', 'ai-key', 'credentials', {
      from: [{ group: 'edgion.io', kind: 'EdgionBackend', namespace: 'apps' }],
      to: [{ group: '', kind: 'Secret', name: 'key' }],
    })],
  }, null, new Set(), true)
  expect(graph.edges.some((edge) => edge.source === 'edgionbackend/apps/provider'
    && edge.target === 'referencegrant/credentials/ai-key' && edge.label === 'granted')).toBe(true)
})


it.each([
  [undefined, true],
  [{}, true],
  [{ ready: null }, true],
  [{ ready: false, serving: true }, true],
  [{ serving: true }, true],
  [{ ready: true }, false],
  [{ ready: true, terminating: true }, false],
])('matches Gateway endpoint readiness for %j', (conditions, unhealthy) => {
  const graph = buildTopologyGraph({ endpointslice: [{
    apiVersion: 'discovery.k8s.io/v1', kind: 'EndpointSlice',
    metadata: { name: 'slice', namespace: 'app' },
    addressType: 'IPv4', endpoints: [{ addresses: ['10.0.0.8'], conditions }],
  } as K8sResource] }, null)
  const endpoint = graph.nodes.find((node) => node.data.kind === 'backend')!
  expect(endpoint.data.unhealthy).toBe(unhealthy)
  expect(endpoint.data.resource.status).toEqual(conditions)
})
