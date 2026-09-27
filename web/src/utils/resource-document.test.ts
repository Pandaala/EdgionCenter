import { describe, expect, it } from 'vitest'
import {
  buildMutationDocument,
  patchDocumentObject,
  mutationDocumentToYaml,
  setDocumentValue,
  withCreateDefaults,
  withoutDocumentPaths,
} from './resource-document'

describe('resource document preservation', () => {
  it.each([
    ['httproute', 'HTTPRoute', 'create'], ['httproute', 'HTTPRoute', 'update'],
    ['grpcroute', 'GRPCRoute', 'create'], ['grpcroute', 'GRPCRoute', 'update'],
  ] as const)('removes Controller ExtensionRef namespaces for %s %s %s', (resourceKind, kind, mode) => {
    const reference = { group: 'edgion.io', kind: 'EdgionPlugins', name: 'auth', future: false }
    const filter = { type: 'ExtensionRef', extensionRef: { ...reference, resolvedNamespace: 'delegated-child' }, future: { resolvedNamespace: 'operator-owned' } }
    const document = {
      apiVersion: 'gateway.networking.k8s.io/v1', kind,
      metadata: { name: 'route', namespace: 'edge', resourceVersion: '7' },
      spec: { rules: [{ filters: [filter], backendRefs: [{ name: 'service', port: 80, filters: [filter] }], future: { resolvedNamespace: 'keep' } }] },
    }
    const result = buildMutationDocument(document, { resourceKind, mode })
    expect(result).toHaveProperty('spec.rules.0.filters.0.extensionRef', reference)
    expect(result).toHaveProperty('spec.rules.0.backendRefs.0.filters.0.extensionRef', reference)
    expect(result).toHaveProperty('spec.rules.0.filters.0.future.resolvedNamespace', 'operator-owned')
    expect(result).toHaveProperty('spec.rules.0.future.resolvedNamespace', 'keep')
    expect(document.spec.rules[0].filters[0].extensionRef.resolvedNamespace).toBe('delegated-child')
  })
  it('rejects missing or unsupported API versions before mutation', () => {
    const base = {
      kind: 'HTTPRoute',
      metadata: { name: 'route', namespace: 'default' },
      spec: {},
    }
    expect(() => buildMutationDocument(base, {
      resourceKind: 'httproute', mode: 'update',
    })).toThrow('apiVersion undefined is not supported')
    expect(() => buildMutationDocument({ ...base, apiVersion: 'gateway.networking.k8s.io/v1beta1' }, {
      resourceKind: 'httproute', mode: 'update',
    })).toThrow('is not supported for HTTPRoute')
  })

  it.each([
    ['referencegrant', 'ReferenceGrant', 'gateway.networking.k8s.io/v1beta1'],
    ['tlsroute', 'TLSRoute', 'gateway.networking.k8s.io/v1alpha3'],
    ['backendtlspolicy', 'BackendTLSPolicy', 'gateway.networking.k8s.io/v1alpha3'],
  ] as const)('accepts the declared %s alternate version', (resourceKind, kind, apiVersion) => {
    expect(buildMutationDocument({ apiVersion, kind, metadata: { name: 'x', namespace: 'default' }, spec: {} }, {
      resourceKind, mode: 'update',
    }).apiVersion).toBe(apiVersion)
  })

  it('strips resolved outbound TLS secrets from EdgionGatewayConfig', () => {
    const result = buildMutationDocument({
      apiVersion: 'edgion.io/v1alpha1',
      kind: 'EdgionGatewayConfig',
      metadata: { name: 'default' },
      spec: {
        outboundTls: {
          verify: false,
          resolvedCaCertificates: [{ data: { ca: 'redacted' } }],
          resolvedClientCertificate: { data: { key: 'redacted' } },
          futureOperatorField: true,
        },
      },
    }, { resourceKind: 'edgiongatewayconfig', mode: 'update' })

    expect(result.spec).toEqual({
      outboundTls: { verify: false, futureOperatorField: true },
    })
  })
  it('adds only missing create-draft defaults and preserves unknown fields and arrays', () => {
    const raw = {
      apiVersion: 'edgion.io/v2',
      kind: 'HTTPRoute',
      metadata: { name: 'existing', futureMeta: { enabled: true } },
      spec: {
        enabled: false,
        rules: [{ name: 'one', futureRuleField: ['a', 'b'] }, { name: 'two' }],
        futureSpec: { nested: 7 },
      },
    }
    const defaults = {
      apiVersion: 'edgion.io/v1',
      kind: 'HTTPRoute',
      metadata: { name: '', namespace: 'default' },
      spec: { enabled: true, rules: [] as unknown[], retries: 3 },
    }

    const result = withCreateDefaults(raw, defaults)

    expect(result).toEqual({
      ...raw,
      metadata: { name: 'existing', namespace: 'default', futureMeta: { enabled: true } },
      spec: { ...raw.spec, retries: 3 },
    })
    expect(result).not.toBe(raw)
    expect(result.spec.rules).not.toBe(raw.spec.rules)
  })

  it('treats null, empty arrays, false, zero, and empty strings as explicit values', () => {
    expect(withCreateDefaults(
      { nullable: null, list: [], enabled: false, count: 0, text: '' },
      { nullable: 'default', list: [1], enabled: true, count: 1, text: 'default' },
    )).toEqual({ nullable: null, list: [], enabled: false, count: 0, text: '' })
  })

  it('updates a nested value without mutating or collapsing sibling rules', () => {
    const raw = {
      spec: {
        rules: [
          { backendRefs: [{ name: 'first', future: true }], retry: { attempts: 3 } },
          { backendRefs: [{ name: 'second' }], sessionPersistence: { type: 'Cookie' } },
        ],
      },
    }

    const result = setDocumentValue(raw, ['spec', 'rules', 0, 'backendRefs', 0, 'name'], 'changed')

    expect(result.spec.rules[0].backendRefs[0]).toEqual({ name: 'changed', future: true })
    expect(result.spec.rules[0].retry).toEqual({ attempts: 3 })
    expect(result.spec.rules[1]).toEqual(raw.spec.rules[1])
    expect(raw.spec.rules[0].backendRefs[0].name).toBe('first')
  })

  it('patches only named object fields', () => {
    const raw = { spec: { tls: { ciphers: ['A'], futureTls: true }, futureSpec: 1 } }
    const result = patchDocumentObject(raw, ['spec', 'tls'], { minTlsVersion: 'Tls12' })

    expect(result).toEqual({
      spec: {
        tls: { ciphers: ['A'], futureTls: true, minTlsVersion: 'Tls12' },
        futureSpec: 1,
      },
    })
  })

  it('builds a lossless operator envelope while stripping server-owned state', () => {
    const raw = {
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'HTTPRoute',
      metadata: {
        name: 'example',
        namespace: 'edge',
        labels: { team: 'gateway' },
        annotations: { future: 'kept' },
        resourceVersion: '12',
        uid: 'server-owned',
        ownerReferences: [{ name: 'owner' }],
        finalizers: ['server.example/finalizer'],
        managedFields: [{ manager: 'controller' }],
      },
      spec: {
        rules: [
          { name: 'one', unknownOperatorField: { enabled: true }, parsedTimeouts: 'remove' },
          { name: 'two', unknownOperatorField: { enabled: false }, parsedTimeouts: 'remove' },
        ],
        futureSpec: ['preserved'],
      },
      status: { conditions: [{ type: 'Accepted', status: 'True' }] },
    }

    expect(buildMutationDocument(raw, {
      mode: 'update',
      resourceKind: 'httproute',
    })).toEqual({
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'HTTPRoute',
      metadata: {
        name: 'example',
        namespace: 'edge',
        labels: { team: 'gateway' },
        annotations: { future: 'kept' },
        resourceVersion: '12',
      },
      spec: {
        rules: [
          { name: 'one', unknownOperatorField: { enabled: true } },
          { name: 'two', unknownOperatorField: { enabled: false } },
        ],
        futureSpec: ['preserved'],
      },
    })
  })

  it('removes wildcard paths from a clone without mutating the API view', () => {
    const raw = { spec: { refs: [{ name: 'one', refDenied: true }, { name: 'two', refDenied: false }] } }
    const result = withoutDocumentPaths(raw, [['spec', 'refs', '*', 'refDenied']])

    expect(result).toEqual({ spec: { refs: [{ name: 'one' }, { name: 'two' }] } })
    expect(raw.spec.refs[0].refDenied).toBe(true)
  })

  it('preserves EndpointSlice top-level operator fields', () => {
    const result = buildMutationDocument({
      apiVersion: 'discovery.k8s.io/v1',
      kind: 'EndpointSlice',
      metadata: { name: 'slice-1', namespace: 'edge', resourceVersion: '9' },
      addressType: 'IPv4',
      endpoints: [{ addresses: ['10.0.0.1'], conditions: { ready: true } }],
      ports: [{ name: 'http', protocol: 'TCP', port: 8080 }],
      status: { ignored: true },
    }, { mode: 'update', resourceKind: 'endpointslice' })

    expect(result).toMatchObject({
      addressType: 'IPv4',
      endpoints: [{ addresses: ['10.0.0.1'], conditions: { ready: true } }],
      ports: [{ name: 'http', protocol: 'TCP', port: 8080 }],
    })
    expect(result).not.toHaveProperty('spec')
    expect(result).not.toHaveProperty('status')
    expect(result.metadata).toHaveProperty('resourceVersion', '9')
  })

  it('preserves ConfigMap data, binaryData, and immutable', () => {
    const result = buildMutationDocument({
      apiVersion: 'v1',
      kind: 'ConfigMap',
      metadata: { name: 'plugin-ca', namespace: 'edge' },
      data: { 'ca.crt': 'PEM', config: '' },
      binaryData: { payload: 'AAE=' },
      immutable: true,
    }, { mode: 'update', resourceKind: 'configmap' })

    expect(result).toMatchObject({
      data: { 'ca.crt': 'PEM', config: '' },
      binaryData: { payload: 'AAE=' },
      immutable: true,
    })
  })

  it('preserves Secret write fields without retaining server metadata or status', () => {
    const result = buildMutationDocument({
      apiVersion: 'v1',
      kind: 'Secret',
      metadata: { name: 'credentials', namespace: 'edge', uid: 'server' },
      data: { token: 'YmFzZTY0' },
      stringData: { password: 'replacement' },
      type: 'Opaque',
      immutable: false,
      status: { internal: true },
    }, { mode: 'update', resourceKind: 'secret' })

    expect(result).toMatchObject({
      data: { token: 'YmFzZTY0' },
      stringData: { password: 'replacement' },
      type: 'Opaque',
      immutable: false,
    })
    expect(result.metadata).not.toHaveProperty('uid')
    expect(result).not.toHaveProperty('status')
  })

  it('preserves the Service spec and drops undeclared top-level fields', () => {
    const result = buildMutationDocument({
      apiVersion: 'v1',
      kind: 'Service',
      metadata: { name: 'backend', namespace: 'edge' },
      spec: { selector: { app: 'backend' }, ports: [{ port: 80 }] },
      endpoints: [{ shouldNot: 'survive' }],
    }, { mode: 'update', resourceKind: 'service' })

    expect(result).toHaveProperty('spec.ports.0.port', 80)
    expect(result).not.toHaveProperty('endpoints')
  })

  it('recursively strips nested plugin runtime and resolved-secret terminals', () => {
    const result = buildMutationDocument({
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionPlugins',
      metadata: { name: 'plugins', namespace: 'edge' },
      spec: {
        requestPlugins: [{
          extAuth: {
            config: {
              endpoint: 'https://auth.example',
              nested: { resolvedSecrets: { token: 'redacted' }, keep: true },
            },
            resolvedCredential: 'redacted',
          },
        }],
      },
    }, { mode: 'update', resourceKind: 'edgionplugins' })

    expect(result).toHaveProperty('spec.requestPlugins.0.extAuth.config.endpoint', 'https://auth.example')
    expect(result).toHaveProperty('spec.requestPlugins.0.extAuth.config.nested.keep', true)
    expect(result).not.toHaveProperty('spec.requestPlugins.0.extAuth.resolvedCredential')
    expect(result).not.toHaveProperty('spec.requestPlugins.0.extAuth.config.nested.resolvedSecrets')
  })

  it.each(['create', 'update'] as const)('removes resolved WAF inputs on %s while preserving operator configuration', (mode) => {
    const operator = {
      policyRef: { name: 'shared-policy', namespace: 'security' },
      activeProfileRef: { name: 'selection' }, mode: 'detectionOnly', priority: 0,
      requestBody: { inspection: 'prefix', prefixSize: '64KiB' },
      futureOperator: { rules: 'nested operator content', selectedProfile: 'keep' },
    }
    const result = buildMutationDocument({
      apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins',
      metadata: { name: 'waf', namespace: 'edge', resourceVersion: '7' },
      spec: {
        waf: { ...operator, rules: 'inspection-only', resolvedPolicy: { defaultProfile: 'base' },
          selectedProfile: 'base', resolvedRefIndices: [0], resolvedBundles: '[redacted]',
          resolutionErrors: [], resolutionWarnings: ['warning'], policyResolved: true,
          selectorResolved: true, ownerNamespace: 'security' },
        requestPlugins: [{ type: 'Mock', config: { status: 200 } }],
      },
    }, { mode, resourceKind: 'edgionplugins' })
    expect(result).toHaveProperty('spec.waf', operator)
    expect(result).toHaveProperty('spec.requestPlugins', [{ type: 'Mock', config: { status: 200 } }])
    if (mode === 'update') expect(result).toHaveProperty('metadata.resourceVersion', '7')
    else expect(result).not.toHaveProperty('metadata.resourceVersion')
  })

  it('fails closed when no resource boundary is registered', () => {
    expect(() => buildMutationDocument({
      apiVersion: 'example.io/v1',
      kind: 'Unknown',
      metadata: { name: 'unknown' },
      spec: { value: true },
    }, { mode: 'update', resourceKind: 'unknown' as never })).toThrow(
      'Resource catalog entry is missing for unknown',
    )
  })

  it('serializes a mutation without dropping explicit empty values', () => {
    const output = mutationDocumentToYaml({
      apiVersion: 'v1',
      kind: 'ConfigMap',
      metadata: { name: 'settings', namespace: 'edge', resourceVersion: '3' },
      data: { empty: '' },
      binaryData: {},
      immutable: false,
      status: { ignored: true },
    }, 'configmap', 'update')

    expect(output).toContain("empty: ''")
    expect(output).toContain('binaryData: {}')
    expect(output).toContain('immutable: false')
    expect(output).toContain("resourceVersion: '3'")
    expect(output).not.toContain('status:')
  })
})
