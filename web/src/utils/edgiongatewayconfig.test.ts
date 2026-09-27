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
    retry: { attempts: 0 },
    requestBody: { defaultMaxBodySize: '32MiB', futureBody: false },
    tcpTimeout: { idleTimeout: '1h', connectTimeout: '10s' },
    loadBalancing: { degradeThreshold: 50 },
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

  it('validates the current degrade threshold and preserves it in mutations', () => {
    for (const value of [0, 50, 100]) {
      const resource = structuredClone(fixture)
      resource.spec.loadBalancing.degradeThreshold = value
      expect(validateEdgionGatewayConfig(resource)).toEqual([])
      expect((yaml.load(toMutationYaml(resource, 'update')) as any).spec.loadBalancing).toEqual({ degradeThreshold: value })
    }
    for (const value of [-1, 101, 0.5, '50', null]) {
      const resource = structuredClone(fixture)
      resource.spec.loadBalancing.degradeThreshold = value
      expect(validateEdgionGatewayConfig(resource)).toContain('spec.loadBalancing.degradeThreshold must be an integer from 0 to 100')
    }
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
      invalid.spec.requestBody.defaultMaxBodySize = value
      expect(validateEdgionGatewayConfig(invalid)).toContain(
        "spec.requestBody.defaultMaxBodySize is invalid (expected a positive byte size such as '32MiB')",
      )
    }

    const minimumPositive = structuredClone(fixture)
    minimumPositive.spec.requestBody.defaultMaxBodySize = '1b'
    minimumPositive.spec.requestBody.defaultMemoryBufferSize = '1b'
    expect(validateEdgionGatewayConfig(minimumPositive)).toEqual([])
  })

  it('resolves request-body overrides and validates the storage deadline', () => {
    const resource = structuredClone(fixture)
    resource.spec.requestBody = { enabled: false, maxMemoryBufferSize: '2MiB', maxBodySize: '1MiB' }
    expect(validateEdgionGatewayConfig(resource).join(' ')).toContain('must not exceed')
    resource.spec.requestBody.maxBodySize = '4MiB'
    expect(validateEdgionGatewayConfig(resource)).toEqual([])
    resource.spec.requestBody.storageOperationTimeout = '0s'
    expect(validateEdgionGatewayConfig(resource)).toContain('spec.requestBody.storageOperationTimeout must be greater than zero')
    resource.spec.requestBody.storageOperationTimeout = '1ms'
    expect(validateEdgionGatewayConfig(resource)).toEqual([])
    resource.spec.maxBodySize = '32MiB'
    expect(validateEdgionGatewayConfig(resource)).toContain('spec.maxBodySize was removed; use spec.requestBody.maxBodySize')
  })

  it('validates current retries and gateway-owned header targets', () => {
    expect(createEmpty().spec.retry).toEqual({ attempts: 2 })
    expect(createEmpty().spec).not.toHaveProperty('maxRetries')
    for (const attempts of [0, 2, 2147483647]) {
      const resource = structuredClone(fixture)
      resource.spec.retry = { attempts }
      expect(validateEdgionGatewayConfig(resource)).toEqual([])
    }
    for (const attempts of [-1, 1.5, 2147483648, '2']) {
      const resource = structuredClone(fixture)
      resource.spec.retry = { attempts }
      expect(validateEdgionGatewayConfig(resource).join(' ')).toContain('spec.retry.attempts')
    }
    for (const remoteIpHeader of ['HOST', 'Content-Length', 'X-Forwarded-For', 'Connection', '', 'bad header', 'x'.repeat(257)]) {
      const resource = structuredClone(fixture)
      resource.spec.forwardedHeaders = { remoteIpHeader }
      expect(validateEdgionGatewayConfig(resource).join(' ')).toContain('remoteIpHeader')
    }
    const resource = structuredClone(fixture)
    resource.spec.forwardedHeaders = { remoteIpHeader: 'X-Client-IP' }
    expect(validateEdgionGatewayConfig(resource)).toEqual([])
  })

  it('preserves allow-list presence and validates plugin-policy precedence', () => {
    const resource = structuredClone(fixture)
    for (const policy of [null, {}, { allow: null }, { allow: [] }, { allow: ['http/RequestId', 'stream/GeoIpLocation', 'tls-route/IpRestriction'] }]) {
      resource.spec.pluginPolicy = policy
      expect(validateEdgionGatewayConfig(resource)).toEqual([])
      expect((yaml.load(toMutationYaml(resource, 'update')) as any).spec.pluginPolicy).toEqual(policy)
    }
    resource.spec.pluginPolicy = { deniedAction: 'bypass', deny: [{ name: 'http/RequestId', blockStatus: 403 }] }
    expect(validateEdgionGatewayConfig(resource)).toContain('pluginPolicy.deny blockStatus must not be set for bypass')
    resource.spec.pluginPolicy.deny[0].action = 'block'
    expect(validateEdgionGatewayConfig(resource)).toEqual([])
    for (const policy of [{ allow: ['http/Missing'] }, { allow: ['http/RequestId', 'http/RequestId'] }, { blockStatus: 200 }, { deny: [null] }, 'invalid']) {
      resource.spec.pluginPolicy = policy
      expect(validateEdgionGatewayConfig(resource).length).toBeGreaterThan(0)
    }
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
