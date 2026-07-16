import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { createWriteOnlyReplacement, fromYaml, normalize, toYaml, validateSecretWrite } from './secret'

describe('Secret mutation adapter', () => {
  it('preserves operator fields and strips server fields without deleting empty values', () => {
    const resource = normalize({
      apiVersion: 'v1',
      kind: 'Secret',
      metadata: {
        name: 'credentials',
        namespace: 'default',
        labels: { app: 'gateway' },
        resourceVersion: '9',
        uid: 'server-owned',
      },
      type: 'Opaque',
      data: { empty: '' },
      stringData: { token: '' },
      immutable: false,
      status: { phase: 'ignored' },
    })

    expect(yaml.load(toYaml(resource, 'update'))).toEqual({
      apiVersion: 'v1',
      kind: 'Secret',
      metadata: {
        name: 'credentials',
        namespace: 'default',
        labels: { app: 'gateway' },
        resourceVersion: '9',
      },
      data: { empty: '' },
      stringData: { token: '' },
      type: 'Opaque',
      immutable: false,
    })
  })

  it('rejects redaction placeholders from both form and YAML submissions', () => {
    const resource = fromYaml(`
apiVersion: v1
kind: Secret
metadata:
  name: credentials
  namespace: default
stringData:
  token: "[redacted]"
`)

    expect(() => toYaml(resource, 'update')).toThrow('contains a redacted value')
  })

  it('builds replacements from metadata only and requires new write-only values', () => {
    const replacement = createWriteOnlyReplacement({ metadata: { name: 'credentials', namespace: 'prod', resourceVersion: '19' } })
    expect(replacement).not.toHaveProperty('data')
    expect(replacement.stringData).toEqual({})
    expect(replacement.metadata.resourceVersion).toBe('19')
    expect(() => validateSecretWrite(replacement)).toThrow('at least one new Secret value')
    replacement.stringData = { password: 'new-value' }
    expect(toYaml(replacement, 'update')).toContain('password: new-value')
  })

  it('rejects any redaction-looking placeholder case-insensitively', () => {
    expect(() => toYaml(fromYaml(`apiVersion: v1\nkind: Secret\nmetadata: {name: s, namespace: n}\nstringData: {token: REDACTED}\n`), 'update')).toThrow('redacted value')
  })
})
