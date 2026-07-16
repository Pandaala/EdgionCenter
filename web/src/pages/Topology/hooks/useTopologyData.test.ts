import { describe, expect, it } from 'vitest'
import type { K8sResource } from '@/api/types'
import { buildTopologyGraph } from './useTopologyData'
import { TOPOLOGY_EDGE_COLORS } from '../components/TopologyCanvas'

function resource(kind: string, name: string, namespace: string | undefined, spec: unknown = {}, status?: unknown): K8sResource {
  return { apiVersion: 'test/v1', kind, metadata: { name, namespace }, spec, status }
}

describe('buildTopologyGraph', () => {
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
