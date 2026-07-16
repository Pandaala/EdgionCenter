import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import {
  gatewayToMutationYaml,
  gatewayToYaml,
  normalizeGateway,
  yamlToGateway,
} from './gateway'
import {
  grpcRouteToMutationYaml,
  grpcRouteToYaml,
  normalizeGRPCRoute,
  yamlToGRPCRoute,
} from './grpcroute'
import { normalizeTCPRoute, tcpRouteToMutationYaml, tcpRouteToYaml, yamlToTCPRoute } from './tcproute'
import { normalizeUDPRoute, udpRouteToMutationYaml } from './udproute'
import { normalizeTLSRoute, tlsRouteToMutationYaml } from './tlsroute'
import {
  createEmpty as createEmptyBackendTls,
  normalize as normalizeBackendTls,
  toMutationYaml as backendTlsToMutationYaml,
} from './backendtlspolicy'
import {
  normalize as normalizeGatewayConfig,
  toMutationYaml as gatewayConfigToMutationYaml,
} from './edgiongatewayconfig'
import { normalize as normalizeLinkSys, toMutationYaml as linkSysToMutationYaml } from './linksys'

function parse(output: string): any {
  return yaml.load(output)
}

const serverMetadata = {
  resourceVersion: '17',
  uid: 'server-uid',
  ownerReferences: [{ name: 'owner' }],
  finalizers: ['controller.edgion.io/finalizer'],
  managedFields: [{ manager: 'controller' }],
  creationTimestamp: '2026-07-15T00:00:00Z',
}

describe('lossless route and gateway adapters', () => {
  it('round-trips every Gateway listener certificate and hidden operator module', () => {
    const fixture = {
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'Gateway',
      metadata: { name: 'edge', namespace: 'prod', ...serverMetadata },
      spec: {
        gatewayClassName: 'edgion',
        addresses: [],
        futureGatewayField: { enabled: false },
        tls: {
          backend: {
            clientCertificateRef: { name: 'default-client', namespace: 'certs' },
            resolvedClientCertificate: { data: { key: 'redacted' } },
          },
          frontend: { default: { validation: { mode: 'AllowValidOnly' } } },
        },
        listeners: [{
          name: 'https',
          port: 443,
          protocol: 'HTTPS',
          allowedRoutes: { namespaces: { from: 'Selector', selector: { matchLabels: { team: 'edge' } } } },
          tls: {
            mode: 'Terminate',
            certificateRefs: [
              { group: '', kind: 'Secret', namespace: 'certs-a', name: 'one' },
              { group: 'cert-manager.io', kind: 'Certificate', namespace: 'certs-b', name: 'two' },
            ],
            options: { 'edgion.io/cert-provider': 'edgion-tls' },
            frontendValidation: {
              mode: 'AllowInsecureFallback',
              caCertificateRefs: [{ group: '', kind: 'Secret', namespace: 'certs', name: 'client-ca' }],
            },
            secrets: [{ data: { key: 'redacted' } }],
            resolvedFrontendCaSecrets: [{ data: { ca: 'redacted' } }],
          },
        }],
      },
      status: { conditions: [{ type: 'Accepted', status: 'True' }] },
    }

    const normalized = normalizeGateway(fixture)
    expect(normalized).toEqual(fixture)
    expect(yamlToGateway(gatewayToYaml(normalized))).toEqual(fixture)

    const mutation = parse(gatewayToMutationYaml(normalized, 'update'))
    expect(mutation.spec.listeners[0].tls.certificateRefs).toEqual(fixture.spec.listeners[0].tls.certificateRefs)
    expect(mutation.spec.listeners[0].tls.options).toEqual(fixture.spec.listeners[0].tls.options)
    expect(mutation.spec.listeners[0].tls.frontendValidation).toEqual(fixture.spec.listeners[0].tls.frontendValidation)
    expect(mutation.spec.listeners[0].allowedRoutes).toEqual(fixture.spec.listeners[0].allowedRoutes)
    expect(mutation.spec.tls.frontend).toEqual(fixture.spec.tls.frontend)
    expect(mutation.spec.tls.backend).not.toHaveProperty('resolvedClientCertificate')
    expect(mutation.spec.listeners[0].tls).not.toHaveProperty('secrets')
    expect(mutation.spec.listeners[0].tls).not.toHaveProperty('resolvedFrontendCaSecrets')
    expect(mutation).not.toHaveProperty('status')
    expect(mutation.metadata).toHaveProperty('resourceVersion', '17')
  })

  it('preserves all GRPCRoute rule fields and strips only runtime paths', () => {
    const fixture = {
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'GRPCRoute',
      metadata: { name: 'grpc', namespace: 'prod', ...serverMetadata },
      spec: {
        parentRefs: [{ group: 'gateway.networking.k8s.io', kind: 'Gateway', namespace: 'infra', name: 'gw', sectionName: 'grpc' }],
        hostnames: [],
        futureSpec: { keep: true },
        rules: [
          {
            name: 'one',
            matches: [{ method: { type: 'Exact', service: 'api.Service' }, headers: [{ type: 'Exact', name: 'x-env', value: '' }] }],
            filters: [{ type: 'ExtensionRef', extensionRef: { group: 'edgion.io', kind: 'EdgionPlugins', name: 'auth' } }],
            backendRefs: [{ group: '', kind: 'Service', namespace: 'backends', name: 'api', port: 50051, weight: 0, refDenied: true }],
            timeouts: { request: '0s', backendRequest: '10s' },
            retry: { attempts: 3, backoff: '100ms', codes: [14] },
            sessionPersistence: { type: 'Cookie', sessionName: 'grpc-session', futureSession: true },
            futureRule: { enabled: false },
            parsedTimeouts: { request: 0 },
          },
          { name: 'two', backendRefs: [], futureRule: ['preserved'] },
        ],
        resolvedRules: [{ internal: true }],
      },
      status: { parents: [] },
    }

    expect(normalizeGRPCRoute(fixture)).toEqual(fixture)
    expect(yamlToGRPCRoute(grpcRouteToYaml(normalizeGRPCRoute(fixture)))).toEqual(fixture)
    const mutation = parse(grpcRouteToMutationYaml(normalizeGRPCRoute(fixture), 'update'))
    expect(mutation.spec.rules).toHaveLength(2)
    expect(mutation.spec.rules[0]).toMatchObject({
      filters: fixture.spec.rules[0].filters,
      timeouts: fixture.spec.rules[0].timeouts,
      retry: fixture.spec.rules[0].retry,
      sessionPersistence: fixture.spec.rules[0].sessionPersistence,
      futureRule: fixture.spec.rules[0].futureRule,
    })
    expect(mutation.spec.rules[0]).not.toHaveProperty('parsedTimeouts')
    expect(mutation.spec.rules[0].backendRefs[0]).not.toHaveProperty('refDenied')
    expect(mutation.spec).not.toHaveProperty('resolvedRules')
  })

  it.each([
    ['TCPRoute', normalizeTCPRoute, tcpRouteToMutationYaml],
    ['UDPRoute', normalizeUDPRoute, udpRouteToMutationYaml],
    ['TLSRoute', normalizeTLSRoute, tlsRouteToMutationYaml],
  ] as const)('preserves multiple %s rules and unknown operator fields', (kind, normalize, mutate) => {
    const fixture = {
      apiVersion: kind === 'TLSRoute' ? 'gateway.networking.k8s.io/v1' : 'gateway.networking.k8s.io/v1alpha2',
      kind,
      metadata: { name: kind.toLowerCase(), namespace: 'prod', ...serverMetadata },
      spec: {
        parentRefs: [{ name: 'gw', sectionName: 'stream', futureParent: true }],
        hostnames: kind === 'TLSRoute' ? [] : undefined,
        futureSpec: { emptyIsMeaningful: '' },
        rules: [
          { name: 'one', backendRefs: [{ name: 'a', namespace: 'backend', port: 9000, weight: 0, refDenied: true }], futureRule: true },
          { name: 'two', backendRefs: [], futureRule: { enabled: false } },
        ],
        resolvedListeners: [{ internal: true }],
      },
      status: { parents: [] },
    }
    const normalized = normalize(fixture as never) as any
    expect(normalized).toEqual(fixture)
    const mutation = parse(mutate(normalized, 'update'))
    expect(mutation.spec.rules).toHaveLength(2)
    expect(mutation.spec.rules[1].futureRule).toEqual({ enabled: false })
    expect(mutation.spec.futureSpec.emptyIsMeaningful).toBe('')
    expect(mutation.spec.rules[0].backendRefs[0]).not.toHaveProperty('refDenied')
    expect(mutation.spec).not.toHaveProperty('resolvedListeners')
  })

  it('keeps explicit empty arrays and strings during route YAML round-trip', () => {
    const tcp = normalizeTCPRoute({
      apiVersion: 'gateway.networking.k8s.io/v1alpha2',
      kind: 'TCPRoute',
      metadata: { name: 'empty', namespace: 'prod' },
      spec: { parentRefs: [], rules: [{ backendRefs: [], future: '' }] },
    })
    expect(yamlToTCPRoute(tcpRouteToYaml(tcp))).toEqual(tcp)
  })
})

describe('lossless policy and system adapters', () => {
  it('creates BackendTLSPolicy v1 and preserves the complete v1 operator contract', () => {
    expect(createEmptyBackendTls().apiVersion).toBe('gateway.networking.k8s.io/v1')
    const fixture = {
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'BackendTLSPolicy',
      metadata: { name: 'backend-tls', namespace: 'prod', ...serverMetadata },
      spec: {
        targetRefs: [{ group: '', kind: 'Service', name: 'api', sectionName: 'https', futureRef: true }],
        validation: {
          hostname: 'api.prod.svc',
          caCertificateRefs: [{ group: '', kind: 'ConfigMap', name: 'ca', namespace: 'security', futureRef: true }],
          subjectAltNames: [
            { type: 'Hostname', hostname: 'api.prod.svc' },
            { type: 'URI', uri: 'spiffe://cluster.local/ns/prod/sa/api' },
          ],
          wellKnownCACertificates: 'System',
          futureValidation: { strict: false },
        },
        options: { 'edgion.io/client-certificate-ref': 'client-cert', future: '' },
        futurePolicy: { enabled: false },
        resolvedCaCertificates: [{ data: { ca: 'redacted' } }],
        resolvedClientCertificate: { data: { tls: 'redacted' } },
        useSystemCa: true,
      },
      status: { ancestors: [] },
    }
    expect(normalizeBackendTls(fixture)).toEqual(fixture)
    const mutation = parse(backendTlsToMutationYaml(normalizeBackendTls(fixture), 'update'))
    expect(mutation.spec.targetRefs).toEqual(fixture.spec.targetRefs)
    expect(mutation.spec.validation).toEqual(fixture.spec.validation)
    expect(mutation.spec.options).toEqual(fixture.spec.options)
    expect(mutation.spec.futurePolicy).toEqual(fixture.spec.futurePolicy)
    expect(mutation.spec).not.toHaveProperty('resolvedCaCertificates')
    expect(mutation.spec).not.toHaveProperty('resolvedClientCertificate')
    expect(mutation.spec).not.toHaveProperty('useSystemCa')
  })

  it('preserves hidden EdgionGatewayConfig modules while stripping server state', () => {
    const fixture = {
      apiVersion: 'edgion.io/v1alpha1',
      kind: 'EdgionGatewayConfig',
      metadata: { name: 'default-config', ...serverMetadata },
      spec: {
        server: { gracePeriodSeconds: 30, hiddenServerField: false },
        linkSys: { webhookMaxResponseBytes: 32768, futureConnectorLimit: 0 },
        outbound: { tls: { validation: { caCertificateRefs: [] } } },
        futureModule: { empty: '' },
      },
      status: { ignored: true },
    }
    expect(normalizeGatewayConfig(fixture)).toEqual(fixture)
    const mutation = parse(gatewayConfigToMutationYaml(normalizeGatewayConfig(fixture), 'update'))
    expect(mutation.spec).toEqual(fixture.spec)
    expect(mutation).not.toHaveProperty('status')
    expect(mutation.metadata).not.toHaveProperty('uid')
  })

  it('preserves hidden LinkSys modules and strips resolved secrets', () => {
    const fixture = {
      apiVersion: 'edgion.io/v1',
      kind: 'LinkSys',
      metadata: { name: 'webhook', namespace: 'prod', ...serverMetadata },
      spec: {
        type: 'webhook',
        futureSpec: { enabled: false },
        config: {
          target: { url: 'https://api.example.com', blockPrivate: true, futureTarget: '' },
          request: {
            path: { template: '/lookup', allowOverride: false },
            headers: { custom: [{ name: 'x-tenant', template: '${ctx:tenant}' }] },
            body: { type: 'json', template: '{"ok":true}' },
          },
          retry: { maxRetries: 2, retryOnBodyFailure: true },
          healthCheck: { passive: { unhealthyThreshold: 3 } },
          futureModule: { values: [] },
          resolvedSecrets: { token: 'redacted' },
        },
      },
      status: { conditions: [] },
    }
    expect(normalizeLinkSys(fixture)).toEqual(fixture)
    const mutation = parse(linkSysToMutationYaml(normalizeLinkSys(fixture), 'update'))
    expect(mutation.spec.futureSpec).toEqual(fixture.spec.futureSpec)
    expect(mutation.spec.config).toMatchObject({
      target: fixture.spec.config.target,
      request: fixture.spec.config.request,
      retry: fixture.spec.config.retry,
      healthCheck: fixture.spec.config.healthCheck,
      futureModule: fixture.spec.config.futureModule,
    })
    expect(mutation.spec.config).not.toHaveProperty('resolvedSecrets')
  })

  it.each([
    ['redis', { endpoints:['redis://r:6379'], auth:{secretRef:{name:'a'},secret:'[redacted]'}, tls:{enabled:true,resolvedCaCertificates:'[redacted]',resolvedClientCertificate:'[redacted]'} }],
    ['elasticsearch', { endpoints:['https://es:9200'], auth:{type:'basic',secretRef:{name:'a'},secret:'[redacted]'}, tls:{enabled:true,resolvedCaCertificates:'[redacted]',resolvedClientCertificate:'[redacted]'} }],
    ['etcd', { endpoints:['https://etcd:2379'], auth:{secretRef:{name:'a'},secret:'[redacted]'}, tls:{enabled:true,resolvedCaCertificates:'[redacted]',resolvedClientCertificate:'[redacted]'} }],
    ['webhook', { target:{url:'https://hook.example.com'}, tls:{enabled:true,resolvedCaCertificates:'[redacted]',resolvedClientCertificate:'[redacted]'}, resolvedSecrets:{a:'[redacted]'} }],
    ['kafka', { brokers:['kafka:9092'], sasl:{username:'u',password:{secretRef:{name:'a'},secret:'[redacted]'}}, tls:{enabled:true,resolvedCaCertificates:'[redacted]',resolvedClientCertificate:'[redacted]'} }],
    ['httpdns', {urlTemplate:'https://dns.example.com/{domain}',response:{kind:'json',ipPath:'ips'},fallback:{type:'system'},connection:{tls:{enabled:true,resolvedCaCertificates:'[redacted]',resolvedClientCertificate:'[redacted]'}}}],
  ])('strips LinkSys %s runtime SecretSlot/TLS fields', (type, config) => {
    const resource = normalizeLinkSys({apiVersion:'edgion.io/v1',kind:'LinkSys',metadata:{name:type,namespace:'prod'},spec:{type,config}})
    const serialized = JSON.stringify(parse(linkSysToMutationYaml(resource,'update')))
    expect(serialized).not.toContain('[redacted]')
    expect(serialized).not.toContain('resolvedCaCertificates')
    expect(serialized).not.toContain('resolvedClientCertificate')
    expect(serialized).not.toContain('"secret"')
  })
})
