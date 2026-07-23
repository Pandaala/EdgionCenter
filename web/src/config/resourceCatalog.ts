import type { ResourceKind, ResourceScope } from '@/api/types'

export type ResourceLifecycle = 'firstClass' | 'restrictedDependency'

export type ResourceArea = 'routes' | 'infrastructure' | 'services' | 'security' | 'plugins' | 'system'
export type MutationPathSegment = string | number | '*' | '**'
export type MutationPath = readonly MutationPathSegment[]

export interface ResourceCatalogEntry {
  kind: ResourceKind
  displayName: string
  apiVersion: string
  /** Explicit alternate wire versions accepted by the Controller. */
  acceptedApiVersions?: readonly string[]
  scope: ResourceScope
  lifecycle: ResourceLifecycle
  area: ResourceArea
  route?: string
  hasConditions: boolean
  /** Operator-owned top-level fields retained in create/update envelopes. */
  operatorTopLevelFields: readonly string[]
  /** Complete known runtime/internal paths removed from mutation envelopes. */
  excludedMutationPaths: readonly MutationPath[]
  /** Explicit UI operations for restricted dependencies; values are never implied from generic CRUD. */
  restrictedOperations?: readonly ('list-keys' | 'create' | 'update' | 'delete')[]
}

type ResourceCatalogBaseEntry = Omit<ResourceCatalogEntry, 'excludedMutationPaths'>

const HTTP_PLUGIN_STAGES = [
  'requestPlugins',
  'upstreamResponseFilterPlugins',
  'upstreamResponseBodyFilterPlugins',
  'upstreamResponsePlugins',
] as const
const HTTP_PLUGIN_INTERNAL_TERMINALS = [
  'refDenied', 'resolvedAuthHeader', 'resolvedPullSecret', 'resolvedUsers', 'resolvedKey',
  'resolvedGroupsByIss', 'resolvedCredentials', 'resolvedCaSecrets',
  'resolvedOidcClientSecret', 'resolvedSessionSecret', 'resolvedCaCertificates',
  'resolvedClientCertificate', 'resolvedCredential', 'resolvedKeys',
  'resolvedSecretValues', 'resolvedSecrets', 'valuesSet', 'wildcardPatterns',
  'compiledRegex', 'compiledPatterns', 'compiledOriginsRegex', 'compiledTemplates',
  'compiledTimingRegex', 'compiledTransformPatterns', 'originsCache', 'ipMatcher',
  'allowMatcher', 'denyMatcher', 'intervalDuration', 'effectiveSlots',
  'rateBytesPerSecond', 'requestTimeoutDuration',
] as const
const HTTP_PLUGIN_INTERNAL_PATHS: readonly MutationPath[] = HTTP_PLUGIN_STAGES.flatMap((stage) => (
  [
    ['spec', stage, '*', 'policyAction'],
    ['spec', stage, '*', 'config', 'allowDegradation'],
    ['spec', stage, '*', 'config', 'allowDegradationTemplate'],
    ...HTTP_PLUGIN_INTERNAL_TERMINALS.map((terminal) => ['spec', stage, '*', '**', terminal]),
  ]
))

const EXCLUDED_MUTATION_PATHS = {
  gatewayclass: [],
  edgiongatewayconfig: [
    ['spec', 'enableReferenceGrantValidation'],
    ['spec', 'outboundTls', 'resolvedCaCertificates'],
    ['spec', 'outboundTls', 'resolvedClientCertificate'],
  ],
  gateway: [
    ['spec', 'tls', 'backend', 'resolvedClientCertificate'],
    ['spec', 'listeners', '*', 'tls', 'secrets'],
    ['spec', 'listeners', '*', 'tls', 'resolvedFrontendCaSecrets'],
  ],
  httproute: [
    ['spec', 'resolvedHostnames'], ['spec', 'resolvedListeners'], ['spec', 'invalidRuleIndices'],
    ['spec', 'resolvedRules'], ['spec', 'delegationIssues'],
    ['spec', 'rules', '*', 'parsedTimeouts'], ['spec', 'rules', '*', 'parsedRetry'],
    ['spec', 'rules', '*', 'parsedForwardRawPath'],
    ['spec', 'rules', '*', 'parsedAllowNonIdempotentRetry'],
    ['spec', 'rules', '*', 'backendRefs', '*', 'backendTlsPolicy'],
    ['spec', 'rules', '*', 'backendRefs', '*', 'refDenied'],
    ['spec', 'rules', '**', 'externalAuth', 'allowDegradation'],
    ['spec', 'rules', '**', 'externalAuth', 'allowDegradationTemplate'],
    ['spec', 'rules', '**', 'requestMirror', 'percentage'],
    ['spec', 'rules', '**', 'requestMirror', 'connectTimeoutMs'],
    ['spec', 'rules', '**', 'requestMirror', 'writeTimeoutMs'],
    ['spec', 'rules', '**', 'requestMirror', 'channelFullTimeoutMs'],
    ['spec', 'rules', '**', 'requestMirror', 'maxBufferedChunks'],
    ['spec', 'rules', '**', 'requestMirror', 'mirrorLog'],
    ['spec', 'rules', '**', 'requestMirror', 'maxConcurrent'],
  ],
  grpcroute: [
    ['spec', 'resolvedHostnames'], ['spec', 'resolvedListeners'], ['spec', 'invalidRuleIndices'],
    ['spec', 'resolvedRules'], ['spec', 'delegationIssues'],
    ['spec', 'rules', '*', 'parsedTimeouts'], ['spec', 'rules', '*', 'parsedRetry'],
    ['spec', 'rules', '*', 'parsedAllowNonIdempotentRetry'],
    ['spec', 'rules', '*', 'backendRefs', '*', 'backendTlsPolicy'],
    ['spec', 'rules', '*', 'backendRefs', '*', 'refDenied'],
  ],
  tcproute: [
    ['spec', 'resolvedListeners'], ['spec', 'rules', '*', 'backendRefs', '*', 'refDenied'],
  ],
  udproute: [
    ['spec', 'resolvedListeners'], ['spec', 'rules', '*', 'backendRefs', '*', 'refDenied'],
  ],
  tlsroute: [
    ['spec', 'resolvedListeners'], ['spec', 'effectiveHostnames'],
    ['spec', 'rules', '*', 'backendRefs', '*', 'refDenied'],
  ],
  service: [],
  endpointslice: [],
  edgiontls: [
    ['spec', 'clientAuth', 'caSecret'], ['spec', 'secret'], ['spec', 'resolvedListeners'],
    ['spec', 'resolvedLogLabels'],
  ],
  referencegrant: [],
  backendtlspolicy: [
    ['spec', 'resolvedCaCertificates'], ['spec', 'resolvedClientCertificate'],
    ['spec', 'useSystemCa'],
  ],
  edgionplugins: HTTP_PLUGIN_INTERNAL_PATHS,
  edgionstreamplugins: [
    ['spec', 'plugins', '*', 'policyAction'],
    ['spec', 'plugins', '*', 'config', '**', 'refDenied'],
    ['spec', 'plugins', '*', 'config', '**', 'ipMatcher'],
    ['spec', 'plugins', '*', 'config', '**', 'allowMatcher'],
    ['spec', 'plugins', '*', 'config', '**', 'denyMatcher'],
    ['spec', 'plugins', '*', 'config', '**', 'intervalDuration'],
    ['spec', 'plugins', '*', 'config', '**', 'effectiveSlots'],
    ['spec', 'tlsRoutePlugins', '*', 'policyAction'],
    ['spec', 'tlsRoutePlugins', '*', 'config', '**', 'refDenied'],
    ['spec', 'tlsRoutePlugins', '*', 'config', '**', 'ipMatcher'],
    ['spec', 'tlsRoutePlugins', '*', 'config', '**', 'allowMatcher'],
    ['spec', 'tlsRoutePlugins', '*', 'config', '**', 'denyMatcher'],
    ['spec', 'tlsRoutePlugins', '*', 'config', '**', 'intervalDuration'],
    ['spec', 'tlsRoutePlugins', '*', 'config', '**', 'effectiveSlots'],
  ],
  edgionconfigdata: [],
  edgionacme: [
    ['spec', 'renewal', 'renewBeforeDays'],
  ],
  linksys: [
    ['spec', 'config', 'resolvedSecrets'],
    ['spec', 'config', 'auth', 'secret'],
    ['spec', 'config', 'tls', 'resolvedCaCertificates'],
    ['spec', 'config', 'tls', 'resolvedClientCertificate'],
    ['spec', 'config', 'sasl', 'password', 'secret'],
    ['spec', 'config', 'connection', 'tls', 'resolvedCaCertificates'],
    ['spec', 'config', 'connection', 'tls', 'resolvedClientCertificate'],
    ['spec', 'config', 'allowDegradation'],
    ['spec', 'config', 'allowDegradationTemplate'],
  ],
  edgionbackendtrafficpolicy: [
    ['spec', 'outlierDetection', 'ejectionSeconds'],
    ['spec', 'outlierDetection', 'maxEjectionSeconds'],
  ],
  secret: [],
  configmap: [],
} as const satisfies Record<ResourceKind, readonly MutationPath[]>

const baseEntries: ResourceCatalogBaseEntry[] = [
  { kind: 'gatewayclass', displayName: 'GatewayClass', apiVersion: 'gateway.networking.k8s.io/v1', scope: 'cluster', lifecycle: 'firstClass', area: 'infrastructure', route: 'infrastructure/gatewayclasses', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'edgiongatewayconfig', displayName: 'EdgionGatewayConfig', apiVersion: 'edgion.io/v1alpha1', scope: 'cluster', lifecycle: 'firstClass', area: 'system', route: 'system/config', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'gateway', displayName: 'Gateway', apiVersion: 'gateway.networking.k8s.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'infrastructure', route: 'infrastructure/gateways', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'httproute', displayName: 'HTTPRoute', apiVersion: 'gateway.networking.k8s.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'routes', route: 'routes/http', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'grpcroute', displayName: 'GRPCRoute', apiVersion: 'gateway.networking.k8s.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'routes', route: 'routes/grpc', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'tcproute', displayName: 'TCPRoute', apiVersion: 'gateway.networking.k8s.io/v1alpha2', scope: 'namespaced', lifecycle: 'firstClass', area: 'routes', route: 'routes/tcp', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'udproute', displayName: 'UDPRoute', apiVersion: 'gateway.networking.k8s.io/v1alpha2', scope: 'namespaced', lifecycle: 'firstClass', area: 'routes', route: 'routes/udp', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'tlsroute', displayName: 'TLSRoute', apiVersion: 'gateway.networking.k8s.io/v1', acceptedApiVersions: ['gateway.networking.k8s.io/v1alpha3'], scope: 'namespaced', lifecycle: 'firstClass', area: 'routes', route: 'routes/tls', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'service', displayName: 'Service', apiVersion: 'v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'services', route: 'services/list', hasConditions: false, operatorTopLevelFields: ['spec'] },
  { kind: 'endpointslice', displayName: 'EndpointSlice', apiVersion: 'discovery.k8s.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'services', route: 'services/endpointslices', hasConditions: false, operatorTopLevelFields: ['addressType', 'endpoints', 'ports'] },
  { kind: 'edgiontls', displayName: 'EdgionTls', apiVersion: 'edgion.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'security', route: 'security/tls', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'referencegrant', displayName: 'ReferenceGrant', apiVersion: 'gateway.networking.k8s.io/v1', acceptedApiVersions: ['gateway.networking.k8s.io/v1beta1'], scope: 'namespaced', lifecycle: 'firstClass', area: 'infrastructure', route: 'infrastructure/referencegrants', hasConditions: false, operatorTopLevelFields: ['spec'] },
  { kind: 'backendtlspolicy', displayName: 'BackendTLSPolicy', apiVersion: 'gateway.networking.k8s.io/v1', acceptedApiVersions: ['gateway.networking.k8s.io/v1alpha3'], scope: 'namespaced', lifecycle: 'firstClass', area: 'security', route: 'security/backendtls', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'edgionplugins', displayName: 'EdgionPlugins', apiVersion: 'edgion.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'plugins', route: 'plugins', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'edgionstreamplugins', displayName: 'EdgionStreamPlugins', apiVersion: 'edgion.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'plugins', route: 'plugins/stream', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'edgionconfigdata', displayName: 'EdgionConfigData', apiVersion: 'edgion.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'plugins', route: 'plugins/metadata', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'edgionacme', displayName: 'EdgionAcme', apiVersion: 'edgion.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'system', route: 'system/acme', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'linksys', displayName: 'LinkSys', apiVersion: 'edgion.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'system', route: 'system/linksys', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'edgionbackendtrafficpolicy', displayName: 'EdgionBackendTrafficPolicy', apiVersion: 'edgion.io/v1', scope: 'namespaced', lifecycle: 'firstClass', area: 'services', route: 'services/backend-traffic-policies', hasConditions: true, operatorTopLevelFields: ['spec'] },
  { kind: 'secret', displayName: 'Secret', apiVersion: 'v1', scope: 'namespaced', lifecycle: 'restrictedDependency', area: 'security', hasConditions: false, operatorTopLevelFields: ['data', 'stringData', 'type', 'immutable'] },
  { kind: 'configmap', displayName: 'ConfigMap', apiVersion: 'v1', scope: 'namespaced', lifecycle: 'restrictedDependency', area: 'security', hasConditions: false, operatorTopLevelFields: ['data', 'binaryData', 'immutable'] },
]

const entries: ResourceCatalogEntry[] = baseEntries.map((entry) => ({
  ...entry,
  excludedMutationPaths: EXCLUDED_MUTATION_PATHS[entry.kind],
}))

for (const entry of entries) {
  if (entry.kind === 'secret' || entry.kind === 'configmap') {
    entry.restrictedOperations = ['list-keys', 'create', 'update']
  }
}

export const RESOURCE_CATALOG: ReadonlyMap<ResourceKind, ResourceCatalogEntry> = new Map(
  entries.map((entry) => [entry.kind, entry]),
)

export function getResourceCatalogEntry(kind: ResourceKind): ResourceCatalogEntry {
  const entry = RESOURCE_CATALOG.get(kind)
  if (!entry) throw new Error(`Resource catalog entry is missing for ${kind}`)
  return entry
}

export function listFirstClassResources(): ResourceCatalogEntry[] {
  return entries.filter((entry) => entry.lifecycle === 'firstClass')
}
