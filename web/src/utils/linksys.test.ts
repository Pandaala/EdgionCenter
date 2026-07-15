import { describe, expect, it } from 'vitest'
import {
  DEFAULT_YAML,
  createEmpty,
  fromYaml,
  toYaml,
  validateLinkSys,
  withWebhookMethod,
  withWebhookUrl,
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
})
