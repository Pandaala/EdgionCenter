import * as yaml from 'js-yaml'
import { describe, expect, it } from 'vitest'
import {
  createEmpty,
  fromYaml,
  normalize,
  parseEdgionByteSize,
  toMutationYaml,
  toYaml,
  validateEdgionGatewayConfig,
} from './edgiongatewayconfig'

const fixture: any = {
  apiVersion: 'edgion.io/v1alpha1',
  kind: 'EdgionGatewayConfig',
  metadata: { name: 'default', annotations: { note: '' }, resourceVersion: '11' },
  spec: {
    server: { threads: 4, workStealing: false, gracePeriodSeconds: 30, gracefulShutdownTimeoutS: 10, upstreamKeepalivePoolSize: 128, errorLog: '', enableCompression: true, downstreamKeepaliveRequestLimit: 0 },
    httpTimeout: { client: { readTimeout: '60s', writeTimeout: '61s', keepaliveTimeout: '75s' }, backend: { defaultConnectTimeout: '5s', defaultRequestTimeout: '60s', defaultIdleTimeout: '300s' } },
    maxRetries: 0,
    maxBodySize: '32MiB',
    tcpTimeout: { idleTimeout: '1h', connectTimeout: '10s' },
    loadBalancing: { panicThreshold: 50 },
    realIp: { trustedIps: [{ name: 'private', description: '', cidrs: ['10.0.0.0/8'], futureGroup: [] }, { name: 'proxy', cidrs: ['192.0.2.1'] }], realIpHeader: 'X-Forwarded-For', recursive: false, maxTrustedHops: 3 },
    securityProtect: { xForwardedForLimit: 200, requireSniHostMatch: false, fallbackSni: '', tlsProxyLogRecord: false, allowLoopbackUpstream: true, rejectDuplicateHost: true },
    globalPluginsRef: [{ name: 'one', namespace: 'prod' }, { name: 'two' }],
    accessLogExtern: {
      unmaskedKeys: {
        header: ['user-agent', 'x-request-id'],
        respHeader: ['content-type'],
        query: ['page'],
        cookie: ['locale'],
        ctx: ['tenant'],
        futureSource: ['future'],
      },
      futurePolicy: false,
    },
    preflightPolicy: { mode: 'all-options', statusCode: 204 },
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
    expect(fromForm.spec.accessLogExtern).toEqual(fixture.spec.accessLogExtern)
    expect(fromForm.spec.futureSpec).toEqual({ empty: [], disabled: false })
    expect(fromForm.spec.pathNormalization).toEqual({ legacyUnknownField: false })
  })

  it('validates duration, body size, CIDR, reference, and DNS resolver constraints', () => {
    expect(validateEdgionGatewayConfig(fixture)).toEqual([])
    const invalid = structuredClone(fixture)
    invalid.spec.tcpTimeout.idleTimeout = 'later'
    invalid.spec.realIp.trustedIps[0].cidrs = ['999.1.1.1/44']
    invalid.spec.outboundTls.validation.caCertificateRefs[0].namespace = ''
    invalid.spec.dnsResolver.linkSysRef = { namespace: '', name: '' }
    const errors = validateEdgionGatewayConfig(invalid).join('\n')
    expect(errors).toContain('not a valid GEP-2257 duration')
    expect(errors).toContain('cidrs[0] is invalid')
    expect(errors).toContain('namespace is required')
    expect(errors).toContain('mutually exclusive')
  })

  it('uses the exact GEP-2257 grammar for every duration surface', () => {
    const durationPaths = [
      ['httpTimeout', 'client', 'readTimeout'],
      ['httpTimeout', 'client', 'writeTimeout'],
      ['httpTimeout', 'client', 'keepaliveTimeout'],
      ['httpTimeout', 'backend', 'defaultConnectTimeout'],
      ['httpTimeout', 'backend', 'defaultRequestTimeout'],
      ['httpTimeout', 'backend', 'defaultIdleTimeout'],
      ['tcpTimeout', 'idleTimeout'],
      ['tcpTimeout', 'connectTimeout'],
      ['dnsResolver', 'cacheTtl'],
    ] as const

    for (const path of durationPaths) {
      const invalid = structuredClone(fixture)
      let target: any = invalid.spec
      for (const segment of path.slice(0, -1)) target = target[segment]
      target[path[path.length - 1]] = '1.5s'
      expect(validateEdgionGatewayConfig(invalid)).toContain(
        `spec.${path.join('.')} is not a valid GEP-2257 duration`,
      )
    }

    for (const value of ['30', '1.5h', '1d', '1 second', '123456s', '1h1m1s1ms1s']) {
      const invalid = structuredClone(fixture)
      invalid.spec.httpTimeout.client.readTimeout = value
      expect(validateEdgionGatewayConfig(invalid).join('\n')).toContain(
        'spec.httpTimeout.client.readTimeout is not a valid GEP-2257 duration',
      )
    }
  })

  it('matches Edgion byte-size units, decimals, trimming, and positive boundary', () => {
    expect(parseEdgionByteSize('1024')).toBe(1024)
    expect(parseEdgionByteSize('1k')).toBe(1024)
    expect(parseEdgionByteSize('1KiB')).toBe(1024)
    expect(parseEdgionByteSize('1.5m')).toBe(1_572_864)
    expect(parseEdgionByteSize(' 512 b ')).toBe(512)
    expect(parseEdgionByteSize('1e3')).toBe(1000)
    expect(parseEdgionByteSize('0x10')).toBeNull()
    expect(parseEdgionByteSize('-1m')).toBeNull()
    expect(parseEdgionByteSize('1e400')).toBeNull()

    for (const value of ['0', '0.5b', '', 'abc', '-1m', '1e400']) {
      const invalid = structuredClone(fixture)
      invalid.spec.maxBodySize = value
      expect(validateEdgionGatewayConfig(invalid)).toContain(
        "spec.maxBodySize is invalid (expected a positive byte size such as '32MiB')",
      )
    }

    const minimumPositive = structuredClone(fixture)
    minimumPositive.spec.maxBodySize = '1b'
    expect(validateEdgionGatewayConfig(minimumPositive)).toEqual([])
  })

  it('does not emit the removed ReferenceGrant field in a newly created document', () => {
    const created = createEmpty()
    expect(created.spec).not.toHaveProperty('enableReferenceGrantValidation')
    expect(yaml.load(toMutationYaml(created, 'create'))).not.toHaveProperty(
      'spec.enableReferenceGrantValidation',
    )
  })
})

function fromYamlDocument() {
  return fromYaml(toYaml(fixture))
}
