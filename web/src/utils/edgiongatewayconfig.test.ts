import * as yaml from 'js-yaml'
import { describe, expect, it } from 'vitest'
import { fromYaml, normalize, toMutationYaml, toYaml, validateEdgionGatewayConfig } from './edgiongatewayconfig'

const fixture: any = {
  apiVersion: 'edgion.io/v1alpha1',
  kind: 'EdgionGatewayConfig',
  metadata: { name: 'default', annotations: { note: '' }, resourceVersion: '11' },
  spec: {
    server: { threads: 4, workStealing: false, gracePeriodSeconds: 30, gracefulShutdownTimeoutS: 10, upstreamKeepalivePoolSize: 128, errorLog: '', enableCompression: true, downstreamKeepaliveRequestLimit: 0 },
    httpTimeout: { client: { readTimeout: '60s', writeTimeout: '61s', keepaliveTimeout: '75s' }, backend: { defaultConnectTimeout: '5s', defaultRequestTimeout: '60s', defaultIdleTimeout: '300s' } },
    maxRetries: 0,
    tcpTimeout: { idleTimeout: '1h', connectTimeout: '10s' },
    loadBalancing: { panicThreshold: 50 },
    realIp: { trustedIps: [{ name: 'private', description: '', cidrs: ['10.0.0.0/8'], futureGroup: [] }, { name: 'proxy', cidrs: ['192.0.2.1'] }], realIpHeader: 'X-Forwarded-For', recursive: false, maxTrustedHops: 3 },
    securityProtect: { xForwardedForLimit: 200, requireSniHostMatch: false, fallbackSni: '', tlsProxyLogRecord: false, allowLoopbackUpstream: true, rejectDuplicateHost: true },
    globalPluginsRef: [{ name: 'one', namespace: 'prod' }, { name: 'two' }],
    preflightPolicy: { mode: 'all-options', statusCode: 204 },
    enableReferenceGrantValidation: true,
    linkSys: { webhookMaxResponseBytes: 32768 },
    outboundTls: { verify: false, validation: { caCertificateRefs: [{ group: '', kind: 'Secret', namespace: 'certs', name: 'ca' }], wellKnownCACertificates: 'System', hostname: 'api.example.com', subjectAltNames: [{ type: 'Hostname', hostname: 'api.example.com' }, { type: 'URI', uri: 'spiffe://cluster/id' }] }, clientCertificateRef: { kind: 'Secret', namespace: 'certs', name: 'client' } },
    dnsResolver: { servers: ['1.1.1.1', '8.8.8.8:53'], cacheTtl: '10s' },
    pathNormalization: { legacyUnknownField: false },
    futureSpec: { empty: [], disabled: false },
  },
  status: { conditions: [{ type: 'Accepted', status: 'True' }] },
}

describe('EdgionGatewayConfig lossless adapter', () => {
  it('round-trips every current section, multi-entry list, empty value, and unknown field', () => {
    expect(fromYaml(toYaml(normalize(fixture)))).toEqual(fixture)
  })

  it('uses one mutation boundary for structured form and YAML', () => {
    const fromForm = yaml.load(toMutationYaml(fixture, 'update')) as any
    const fromYaml = yaml.load(toMutationYaml(fromYamlDocument(), 'update')) as any
    expect(fromYaml).toEqual(fromForm)
    expect(fromForm.status).toBeUndefined()
    expect(fromForm.metadata.resourceVersion).toBe('11')
    expect(fromForm.spec.realIp.trustedIps).toHaveLength(2)
    expect(fromForm.spec.globalPluginsRef).toHaveLength(2)
    expect(fromForm.spec.futureSpec).toEqual({ empty: [], disabled: false })
    expect(fromForm.spec.pathNormalization).toEqual({ legacyUnknownField: false })
  })

  it('validates duration, CIDR, reference, and DNS resolver constraints', () => {
    expect(validateEdgionGatewayConfig(fixture)).toEqual([])
    const invalid = structuredClone(fixture)
    invalid.spec.tcpTimeout.idleTimeout = 'later'
    invalid.spec.realIp.trustedIps[0].cidrs = ['999.1.1.1/44']
    invalid.spec.outboundTls.validation.caCertificateRefs[0].namespace = ''
    invalid.spec.dnsResolver.linkSysRef = { namespace: '', name: '' }
    const errors = validateEdgionGatewayConfig(invalid).join('\n')
    expect(errors).toContain('not a valid duration')
    expect(errors).toContain('cidrs[0] is invalid')
    expect(errors).toContain('namespace is required')
    expect(errors).toContain('mutually exclusive')
  })
})

function fromYamlDocument() {
  return fromYaml(toYaml(fixture))
}
