import { describe, expect, it } from 'vitest'
import {
  DEFAULT_YAML,
  createEmpty,
  fromYaml,
  toYaml,
  validateLinkSys,
  withWebhookMethod,
  withWebhookUrl,
  withWebhookServiceTarget,
  LINKSYS_RUST_FIELD_MATRIX,
  toMutationYaml,
} from './linksys'

describe('LinkSys YAML utilities', () => {
  it('serializes the current tagged config wire shape', () => {
    const resource = createEmpty()
    resource.metadata.name = 'redis-test'
    resource.spec.config = {
      endpoints: ['redis://127.0.0.1:6379'],
      db: 0,
      auth: { secretRef: { name: 'redis-credentials' } },
      topology: { mode: 'standalone' },
    }

    const serialized = toYaml(resource)

    expect(serialized).toContain('type: redis')
    expect(serialized).toContain('config:')
    expect(serialized).toContain('endpoints:')
    expect(serialized).toContain('secretRef:')
    expect(serialized).not.toContain('addresses:')
    expect(serialized).not.toContain('password:')
  })

  it('round-trips a webhook target and request method', () => {
    const resource = fromYaml(`apiVersion: edgion.io/v1
kind: LinkSys
metadata:
  name: webhook-test
  namespace: default
spec:
  type: webhook
  config:
    target:
      url: https://example.com
    request:
      path:
        template: /lookup
      method:
        template: POST
      headers:
        custom:
          - name: Authorization
            template: "Bearer \${secretRef:default/webhook-token:token}"
    timeoutMs: 5000
`)

    expect(resource.spec.type).toBe('webhook')
    expect(resource.spec.config).toMatchObject({
      target: { url: 'https://example.com' },
      request: { method: { template: 'POST' } },
      timeoutMs: 5000,
    })
    expect(toYaml(resource)).toContain('url: https://example.com')
    expect(toYaml(resource)).toContain('Authorization')
    expect(toYaml(resource)).toContain('secretRef:default/webhook-token:token')
  })

  it('preserves hidden Webhook fields when the form changes URL or method', () => {
    const original = {
      target: { url: 'https://old.example.com', blockPrivate: true },
      request: {
        path: { template: '/lookup' },
        method: { template: 'GET' },
        headers: { custom: [{ name: 'Authorization', template: 'Bearer token' }] },
        body: { type: 'json', template: '{}' },
      },
      timeoutMs: 5000,
    }

    const updated = withWebhookMethod(withWebhookUrl(original, 'https://new.example.com'), 'POST')

    expect(updated.target).toEqual({ url: 'https://new.example.com', blockPrivate: true })
    expect(updated.request).toMatchObject({
      path: { template: '/lookup' },
      method: { template: 'POST' },
      headers: original.request.headers,
      body: original.request.body,
    })
  })

  it('makes URL and Service targets strictly mutually exclusive in helpers and validation', () => {
    const base:any={target:{name:'svc',namespace:'prod',port:8080},request:{},timeoutMs:5000}
    expect(withWebhookUrl(base,'https://hook.example.com').target).toEqual({url:'https://hook.example.com',blockPrivate:undefined})
    expect(withWebhookServiceTarget({ ...base, target:{url:'https://hook.example.com',blockPrivate:true} },{name:'svc',port:8080}).target).toMatchObject({url:undefined,blockPrivate:undefined,name:'svc',port:8080})
    const invalid=fromYaml('apiVersion: edgion.io/v1\nkind: LinkSys\nmetadata: {name: hook, namespace: prod}\nspec: {type: webhook, config: {target: {url: https://hook.example.com, group: x}, request: {}, timeoutMs: 5000}}\n')
    expect(()=>validateLinkSys(invalid)).toThrow('exactly one URL or Service')
  })

  it('keeps the default YAML accepted shape', () => {
    expect(fromYaml(DEFAULT_YAML).spec.config).toMatchObject({
      endpoints: ['redis://127.0.0.1:6379'],
      topology: { mode: 'standalone' },
    })
  })

  it('supports Kafka and HTTP DNS wire shapes', () => {
    const kafka = fromYaml(`apiVersion: edgion.io/v1
kind: LinkSys
metadata: { name: kafka-test, namespace: default }
spec:
  type: kafka
  config:
    brokers: [kafka:9092]
`)
    const httpDns = fromYaml(`apiVersion: edgion.io/v1
kind: LinkSys
metadata: { name: httpdns-test, namespace: default }
spec:
  type: httpdns
  config:
    urlTemplate: https://dns.example.com/resolve?host={domain}
    response: { kind: json, ipPath: ips }
    fallback: { type: system }
`)

    expect(() => validateLinkSys(kafka)).not.toThrow()
    expect(() => validateLinkSys(httpDns)).not.toThrow()
    expect(toYaml(kafka)).toContain('brokers:')
    expect(toYaml(httpDns)).toContain('urlTemplate:')
  })

  it('rejects incomplete form-mode resources before calling the backend', () => {
    const empty = createEmpty()
    empty.metadata.name = 'invalid'
    expect(() => validateLinkSys(empty)).toThrow('at least one endpoint')

    empty.spec.config = {
      endpoints: ['redis://127.0.0.1:6379'],
      auth: { secretRef: { name: '' } },
    }
    expect(() => validateLinkSys(empty)).toThrow('requires a Secret name')
  })

  it('locks the Rust-derived field matrix and round-trips deep webhook configuration', () => {
    expect(LINKSYS_RUST_FIELD_MATRIX.webhook).toContain('healthCheck')
    expect(LINKSYS_RUST_FIELD_MATRIX.webhook).not.toContain('allowDegradation')
    expect(LINKSYS_RUST_FIELD_MATRIX.webhook).not.toContain('allowDegradationTemplate')
    expect(LINKSYS_RUST_FIELD_MATRIX.etcd).toContain('maxCallRecvSize')
    const resource = fromYaml(`apiVersion: edgion.io/v1
kind: LinkSys
metadata: {name: resolver, namespace: prod}
spec:
  type: webhook
  config:
    target: {url: https://resolver.example.com, blockPrivate: true}
    tls: {enabled: true, verify: true, validation: {wellKnownCACertificates: System}}
    timeoutMs: 3000
    timeoutMsTemplate: "\${ctx:timeout}"
    retry: {maxRetries: 2, retryDelayMs: 100, retryOnStatus: [503], backoffPolicy: exponential, maxDelayMs: 1000}
    rateLimit: {rate: 100, windowSec: 1}
    healthCheck: {active: {path: /healthz, intervalSec: 10, timeoutMs: 2000, healthyThreshold: 1, unhealthyThreshold: 3}, passive: {unhealthyThreshold: 3, failureStatusCodes: [500], countTimeout: true, backoff: {initialSec: 5, multiplier: 2, maxSec: 60}}}
    maxResponseBytes: 4096
    allowDegradation: true
    allowDegradationTemplate: "\${ctx:degrade}"
    success: {statusCodes: [200], body: [{pointer: /code, equals: 0}]}
    statusOnError: 503
    request:
      path: {template: /lookup, allowOverride: false}
      method: {template: POST}
      args: {origin: [{name: tenant, presence: required}], custom: []}
      headers: {forwardAll: false, origin: [], custom: [{name: Authorization, template: "Bearer \${secretRef:prod/token:value}"}]}
      cookies: {origin: [], custom: []}
      body: {type: json, template: '{"id":"\${ctx:id}"}'}
`)
    const output = toMutationYaml(resource, 'update')
    expect(output).toContain('healthCheck:')
    expect(output).toContain('retryOnStatus:')
    expect(output).not.toContain('allowDegradation')
    expect(output).toContain('healthCheck:')
    expect(output).not.toContain('resolvedSecrets:')
  })
})
