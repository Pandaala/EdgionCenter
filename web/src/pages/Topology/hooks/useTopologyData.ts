import { useCallback, useMemo } from 'react'
import { useParams } from 'react-router-dom'
import { useQueries } from '@tanstack/react-query'
import { clusterResourceApi, resourceApi } from '@/api/resources'
import type { K8sResource, ResourceKind } from '@/api/types'
import { resourceIssues } from '@/utils/controller-observability'

export type TopologyEdgeState = 'resolved' | 'unresolved' | 'conflict' | 'unavailable' | 'unknown'

export interface TopoNode {
  id: string
  data: {
    kind: string
    name: string
    namespace?: string
    resource: K8sResource
    layer: number
    unresolved?: boolean
    rejected?: boolean
    conflict?: boolean
    synthetic?: boolean
    unavailable?: boolean
    unhealthy?: boolean
    [key: string]: unknown
  }
}

export interface TopoEdge {
  id: string
  source: string
  target: string
  label?: string
  state: TopologyEdgeState
  dashed?: boolean
}

interface TopologyData {
  nodes: TopoNode[]
  edges: TopoEdge[]
  namespaces: string[]
  isLoading: boolean
  isError: boolean
  partialErrors: string[]
  refetch: () => void
}

export type TopologyResources = Partial<Record<ResourceKind, K8sResource[]>>

const ROUTE_KINDS = new Set<ResourceKind>(['httproute', 'grpcroute', 'tcproute', 'udproute', 'tlsroute'])
const CLUSTER_KINDS = new Set<ResourceKind>(['gatewayclass', 'edgiongatewayconfig'])
const RESTRICTED_KINDS = new Set<ResourceKind>(['secret', 'configmap'])

export const TOPOLOGY_KINDS: readonly ResourceKind[] = [
  'edgiongatewayconfig', 'gatewayclass', 'gateway',
  'httproute', 'grpcroute', 'tcproute', 'udproute', 'tlsroute',
  'service', 'endpointslice', 'edgionbackendtrafficpolicy', 'backendtlspolicy',
  'edgionplugins', 'edgionstreamplugins', 'edgionconfigdata',
  'edgiontls', 'edgionacme', 'linksys', 'referencegrant', 'secret', 'configmap',
]

const KIND_ALIASES: Record<string, ResourceKind> = {
  gatewayclass: 'gatewayclass', gateway: 'gateway',
  edgiongatewayconfig: 'edgiongatewayconfig',
  httproute: 'httproute', grpcroute: 'grpcroute', tcproute: 'tcproute',
  udproute: 'udproute', tlsroute: 'tlsroute', service: 'service',
  endpointslice: 'endpointslice', backendtlspolicy: 'backendtlspolicy',
  edgionbackendtrafficpolicy: 'edgionbackendtrafficpolicy',
  edgionplugins: 'edgionplugins', edgionstreamplugins: 'edgionstreamplugins',
  edgionconfigdata: 'edgionconfigdata', edgiontls: 'edgiontls',
  edgionacme: 'edgionacme', linksys: 'linksys', secret: 'secret', configmap: 'configmap',
}

function normalizeKind(value: unknown, fallback?: ResourceKind): ResourceKind | undefined {
  if (typeof value !== 'string') return fallback
  return KIND_ALIASES[value.toLowerCase()] ?? fallback
}

function nodeId(kind: ResourceKind | 'backend' | 'unknown' | 'referencegrant', namespace: string | undefined, name: string): string {
  return `${kind}/${namespace ?? '_cluster'}/${name}`
}

function layerFor(kind: ResourceKind | 'backend' | 'unknown' | 'referencegrant'): number {
  if (kind === 'gatewayclass' || kind === 'gateway') return 0
  if (ROUTE_KINDS.has(kind as ResourceKind)) return 1
  if (kind === 'service' || kind === 'edgionbackendtrafficpolicy' || kind === 'backendtlspolicy') return 2
  if (kind === 'edgionplugins' || kind === 'edgionstreamplugins') return 3
  if (kind === 'edgiongatewayconfig' || kind === 'edgionconfigdata' || kind === 'linksys' || kind === 'edgiontls' || kind === 'edgionacme') return 4
  return 5
}

function resourceFlags(resource: K8sResource) {
  const issues = resourceIssues(resource)
  const rejected = issues.includes('rejected')
  const conflict = issues.includes('conflict')
  return { rejected, conflict }
}

interface Reference {
  kind: ResourceKind | 'unknown'
  name: string
  namespace?: string
  label: string
  reverse?: boolean
}

const EXPECTED_GROUP: Partial<Record<ResourceKind, string>> = {
  service: '', secret: '', configmap: '', endpointslice: 'discovery.k8s.io',
  gateway: 'gateway.networking.k8s.io', gatewayclass: 'gateway.networking.k8s.io',
  httproute: 'gateway.networking.k8s.io', grpcroute: 'gateway.networking.k8s.io',
  tcproute: 'gateway.networking.k8s.io', udproute: 'gateway.networking.k8s.io', tlsroute: 'gateway.networking.k8s.io',
  edgionplugins: 'edgion.io', edgionstreamplugins: 'edgion.io', edgionconfigdata: 'edgion.io',
  edgiontls: 'edgion.io', linksys: 'edgion.io', edgiongatewayconfig: 'edgion.io',
}

function refFrom(value: unknown, kind: ResourceKind, namespace: string | undefined, label: string): Reference | undefined {
  if (typeof value === 'string') {
    const [first, second] = value.includes('/') ? value.split('/', 2) : [undefined, value]
    return second ? { kind, name: second, namespace: first ?? namespace, label } : undefined
  }
  if (typeof value !== 'object' || value === null) return undefined
  const ref = value as Record<string, unknown>
  if (typeof ref.name !== 'string' || ref.name.length === 0) return undefined
  const resolvedKind = normalizeKind(ref.kind, kind)
  const explicitUnknownKind = typeof ref.kind === 'string' && !normalizeKind(ref.kind)
  const group = typeof ref.group === 'string' ? ref.group : undefined
  const expected = resolvedKind ? EXPECTED_GROUP[resolvedKind] : undefined
  const normalizedGroup = group === 'core' ? '' : group
  const invalidGroup = normalizedGroup !== undefined && expected !== undefined && normalizedGroup !== expected
  return {
    kind: explicitUnknownKind || invalidGroup ? 'unknown' : resolvedKind ?? kind,
    name: ref.name,
    namespace: typeof ref.namespace === 'string' ? ref.namespace : namespace,
    label: typeof ref.sectionName === 'string' ? `${label}#${ref.sectionName}` : label,
  }
}

function collectApprovedReferences(value: unknown, namespace: string | undefined, result: Reference[], seen = new Set<unknown>()) {
  if (typeof value !== 'object' || value === null || seen.has(value)) return
  seen.add(value)
  if (Array.isArray(value)) {
    value.forEach((item) => collectApprovedReferences(item, namespace, result, seen))
    return
  }
  const record = value as Record<string, unknown>
  const fieldKinds: Record<string, ResourceKind> = {
    secretRef: 'secret', secretRefs: 'secret', caSecretRef: 'secret',
    caSecretRefs: 'secret', authHeaderSecretRef: 'secret', clientSecretRef: 'secret', sessionSecretRef: 'secret',
    caCertificateRefs: 'secret', clientCertificateRef: 'secret', pullSecretRef: 'secret',
    privateKeySecretRef: 'secret', certificateRefs: 'secret', credentialSecretRef: 'secret',
    configMapRef: 'configmap', configMapRefs: 'configmap',
    linksysRef: 'linksys', linksysRefs: 'linksys', linkSysRef: 'linksys', linkSysRefs: 'linksys',
    redisRef: 'linksys',
    allowRefs: 'edgionconfigdata', denyRefs: 'edgionconfigdata', activeProfileRef: 'edgionconfigdata',
    overrideRef: 'edgionconfigdata', configDataRef: 'edgionconfigdata', configDataRefs: 'edgionconfigdata',
  }
  for (const [key, child] of Object.entries(record)) {
    const refKind = fieldKinds[key]
    if (refKind) {
      const values = Array.isArray(child) ? child : [child]
      values.forEach((item) => {
        const flattenedSecret = key === 'authHeaderSecretRef' && typeof item === 'object' && item !== null
          ? ((item as Record<string, unknown>).secret ?? item) : item
        const ref = refFrom(flattenedSecret, refKind, namespace, key)
        if (ref) result.push(ref)
      })
    }
    collectApprovedReferences(child, namespace, result, seen)
  }
}

function referencesFor(kind: ResourceKind, resource: K8sResource): Reference[] {
  const namespace = resource.metadata.namespace
  const refs: Reference[] = []
  const spec = resource.spec ?? {}
  const annotations = resource.metadata.annotations ?? {}
  const annotationRefs = (key: string, targetKind: ResourceKind, label: string) => {
    const raw = annotations[key]
    if (!raw) return
    raw.split(/[\s,]+/).filter(Boolean).forEach((token) => {
      const ref = refFrom(token, targetKind, namespace, label)
      if (ref) refs.push(ref)
    })
  }
  annotationRefs('edgion.io/edgion-plugins', 'edgionplugins', 'plugin annotation')
  annotationRefs('edgion.io/edgion-stream-plugins', 'edgionstreamplugins', 'stream plugin annotation')
  annotationRefs('edgion.io/dns-resolver-ref', 'linksys', 'DNS resolver')

  if (kind === 'gatewayclass' && spec.parametersRef) {
    const ref = refFrom(spec.parametersRef, 'edgiongatewayconfig', undefined, 'parameters')
    if (ref) refs.push(ref)
  }
  if (kind === 'edgiongatewayconfig') {
    for (const plugin of spec.globalPluginsRef ?? []) {
      const ref = refFrom(plugin, 'edgionplugins', plugin.namespace ?? 'default', 'global plugin')
      if (ref) refs.push(ref)
    }
  }
  if (kind === 'gateway' && typeof spec.gatewayClassName === 'string') {
    refs.push({ kind: 'gatewayclass', name: spec.gatewayClassName, label: 'class', reverse: true })
  }
  if (kind === 'gateway') {
    for (const listener of spec.listeners ?? []) {
      for (const certificate of listener.tls?.certificateRefs ?? []) {
        const ref = refFrom(certificate, 'secret', namespace, `certificate#${listener.name ?? ''}`)
        if (ref) refs.push(ref)
      }
    }
  }
  if (ROUTE_KINDS.has(kind)) {
    for (const parent of spec.parentRefs ?? []) {
      const ref = refFrom(parent, 'gateway', namespace, 'parent')
      if (ref) refs.push({ ...ref, reverse: true })
    }
    for (const rule of spec.rules ?? []) {
      for (const backend of rule.backendRefs ?? []) {
        const ref = refFrom(backend, 'service', namespace, backend.port ? `backend:${backend.port}` : 'backend')
        if (ref) refs.push(ref)
      }
      for (const filter of rule.filters ?? []) {
        if (filter?.type === 'ExtensionRef') {
          const ref = refFrom(filter.extensionRef, 'edgionplugins', namespace, 'plugin')
          if (ref) refs.push(ref)
        }
      }
    }
  }
  if (kind === 'edgiontls') {
    for (const parent of spec.parentRefs ?? []) {
      const ref = refFrom(parent, 'gateway', namespace, 'TLS')
      if (ref) refs.push({ ...ref, reverse: true })
    }
    for (const [value, label] of [[spec.secretRef, 'certificate'], [spec.clientAuth?.caSecretRef, 'client CA']] as const) {
      const ref = refFrom(value, 'secret', namespace, label)
      if (ref) refs.push(ref)
    }
  }
  if (kind === 'edgionacme') {
    for (const [value, label] of [[spec.privateKeySecretRef, 'account'], [spec.externalAccountBinding?.keySecretRef, 'EAB credential'], [spec.challenge?.credentialRef, 'DNS credential']] as const) {
      const ref = refFrom(value, 'secret', namespace, label)
      if (ref) refs.push(ref)
    }
    if (typeof spec.storage?.secretName === 'string') {
      refs.push({ kind: 'secret', name: spec.storage.secretName, namespace: spec.storage.secretNamespace ?? namespace, label: 'certificate' })
    }
    if (spec.autoEdgionTls?.enabled !== false) {
      refs.push({ kind: 'edgiontls', name: spec.autoEdgionTls?.name ?? `acme-${resource.metadata.name}`, namespace, label: 'managed TLS' })
      for (const parent of spec.autoEdgionTls?.parentRefs ?? []) {
        const ref = refFrom(parent, 'gateway', namespace, 'ACME TLS')
        if (ref) refs.push({ ...ref, reverse: true })
      }
    }
  }
  if (kind === 'edgionbackendtrafficpolicy' || kind === 'backendtlspolicy') {
    for (const target of spec.targetRefs ?? []) {
      const ref = refFrom(target, 'service', namespace, 'policy')
      if (ref) refs.push({ ...ref, reverse: true })
    }
    if (kind === 'backendtlspolicy') {
      for (const certificate of spec.validation?.caCertificateRefs ?? []) {
        const ref = refFrom(certificate, 'secret', namespace, 'backend CA')
        if (ref) refs.push(ref)
      }
      const clientCertificate = spec.options?.['edgion.io/client-certificate-ref']
      const ref = refFrom(clientCertificate, 'secret', namespace, 'client certificate')
      if (ref) refs.push(ref)
    }
  }
  if (kind === 'referencegrant') {
    for (const from of spec.from ?? []) {
      const ref = refFrom({ ...from, name: '*' }, 'unknown' as ResourceKind, from.namespace, 'allows from')
      if (ref) refs.push({ ...ref, reverse: true })
    }
    for (const to of spec.to ?? []) {
      const fallback = normalizeKind(to.kind)
      const ref = fallback ? refFrom({ ...to, name: to.name || '*' }, fallback, namespace, 'allows to') : undefined
      if (ref) refs.push(ref)
    }
  }
  if (kind === 'edgionplugins' || kind === 'edgionstreamplugins' || kind === 'linksys') {
    collectApprovedReferences(spec, namespace, refs)
  }
  return refs.filter((ref, index) => refs.findIndex((item) => (
    item.kind === ref.kind && item.name === ref.name && item.namespace === ref.namespace
    && item.label === ref.label && item.reverse === ref.reverse
  )) === index)
}

export function buildTopologyGraph(resources: TopologyResources, namespaceFilter: string | null, unavailableKinds = new Set<ResourceKind>(), referenceGrantValidation: boolean | 'unknown' = 'unknown'): Pick<TopologyData, 'nodes' | 'edges' | 'namespaces'> {
  const nodes: TopoNode[] = []
  const nodeMap = new Map<string, TopoNode>()
  const namespaces = new Set<string>()

  for (const kind of TOPOLOGY_KINDS) {
    for (const resource of resources[kind] ?? []) {
      const name = resource.metadata?.name
      if (!name) continue
      const namespace = resource.metadata.namespace
      if (namespace) namespaces.add(namespace)
      const id = nodeId(kind, namespace, name)
      const node: TopoNode = {
        id,
        data: { kind, name, namespace, resource, layer: layerFor(kind), ...resourceFlags(resource) },
      }
      nodes.push(node)
      nodeMap.set(id, node)
    }
  }

  const edges: TopoEdge[] = []
  const edgeIds = new Set<string>()
  const addEdge = (source: string, target: string, label: string, state: TopologyEdgeState) => {
    const id = `${source}->${target}:${label}`
    if (edgeIds.has(id)) return
    edgeIds.add(id)
    edges.push({ id, source, target, label, state, dashed: state !== 'resolved' })
  }
  const ensureTarget = (ref: Reference, owner: TopoNode): TopoNode => {
    const id = nodeId(ref.kind, ref.kind !== 'unknown' && CLUSTER_KINDS.has(ref.kind) ? undefined : ref.namespace ?? owner.data.namespace, ref.name)
    const existing = nodeMap.get(id)
    if (existing) return existing
    const namespace = ref.kind !== 'unknown' && CLUSTER_KINDS.has(ref.kind) ? undefined : ref.namespace ?? owner.data.namespace
    const unavailable = ref.kind !== 'unknown' && unavailableKinds.has(ref.kind)
    const placeholder: TopoNode = {
      id,
      data: {
        kind: ref.kind,
        name: ref.name,
        namespace,
        layer: layerFor(ref.kind),
        unresolved: !unavailable,
        unavailable,
        resource: { apiVersion: '', kind: ref.kind, metadata: { name: ref.name, namespace } },
      },
    }
    nodes.push(placeholder)
    nodeMap.set(id, placeholder)
    return placeholder
  }

  for (const owner of [...nodes]) {
    if (owner.data.unresolved || owner.data.synthetic) continue
    for (const ref of referencesFor(owner.data.kind as ResourceKind, owner.data.resource)) {
      const target = ensureTarget(ref, owner)
      const state: TopologyEdgeState = target.data.unavailable ? 'unavailable' : target.data.kind === 'unknown' ? 'unknown' : target.data.unresolved
        ? 'unresolved'
        : owner.data.conflict || target.data.conflict ? 'conflict' : 'resolved'
      addEdge(ref.reverse ? target.id : owner.id, ref.reverse ? owner.id : target.id, ref.label, state)
    }
  }

  // Project ReferenceGrant authorization for concrete cross-namespace edges.
  // This is observational: Controllers with validation disabled may still
  // accept the reference, but the graph makes the matching grant (or absence)
  // explicit instead of silently treating it as a same-namespace edge.
  const grants = resources.referencegrant ?? []
  const apiGroupFor = (kind: string) => kind === 'service' || kind === 'secret' || kind === 'configmap'
    ? '' : ROUTE_KINDS.has(kind as ResourceKind) || kind === 'gateway'
      ? 'gateway.networking.k8s.io' : kind.startsWith('edgion') || kind === 'linksys' ? 'edgion.io' : ''
  const displayKind = (kind: string) => KIND_ALIASES[kind]?.replace(/^./, (value) => value.toUpperCase()) ?? kind
  for (const edge of [...edges]) {
    const source = nodeMap.get(edge.source); const target = nodeMap.get(edge.target)
    if (!source?.data.namespace || !target?.data.namespace || source.data.namespace === target.data.namespace
      || source.data.kind === 'referencegrant' || target.data.kind === 'referencegrant'
      || source.data.unavailable || target.data.unavailable || source.data.kind === 'unknown' || target.data.kind === 'unknown') continue
    if (referenceGrantValidation === false) continue
    if (referenceGrantValidation === 'unknown') {
      const name = `grant-check-unavailable:${target.data.kind}/${target.data.name}`
      const checkId = nodeId('referencegrant', target.data.namespace, name)
      if (!nodeMap.has(checkId)) {
        const check: TopoNode = { id: checkId, data: { kind: 'referencegrant', name, namespace: target.data.namespace, layer: 4, unavailable: true, synthetic: true, resource: { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'ReferenceGrantCheck', metadata: { name, namespace: target.data.namespace } } } }
        nodes.push(check); nodeMap.set(checkId, check)
      }
      addEdge(source.id, checkId, 'grant check unavailable', 'unknown'); addEdge(checkId, target.id, 'unknown', 'unknown')
      continue
    }
    const matching = grants.find((grant) => grant.metadata.namespace === target.data.namespace
      && (grant.spec?.from ?? []).some((from: any) => from.namespace === source.data.namespace
        && from.kind.toLowerCase() === source.data.kind && (from.group === apiGroupFor(source.data.kind) || (from.group === 'core' && apiGroupFor(source.data.kind) === '')))
      && (grant.spec?.to ?? []).some((to: any) => to.kind.toLowerCase() === target.data.kind
        && (!to.name || to.name === target.data.name) && (to.group === apiGroupFor(target.data.kind) || (to.group === 'core' && apiGroupFor(target.data.kind) === ''))))
    if (matching) {
      const grantId = nodeId('referencegrant', matching.metadata.namespace, matching.metadata.name)
      addEdge(source.id, grantId, 'granted', 'resolved'); addEdge(grantId, target.id, 'allows', 'resolved')
    } else {
      const name = `denied:${displayKind(target.data.kind)}/${target.data.name}`
      const deniedId = nodeId('referencegrant', target.data.namespace, name)
      if (!nodeMap.has(deniedId)) {
        const denied: TopoNode = { id: deniedId, data: { kind: 'referencegrant', name, namespace: target.data.namespace, layer: 4, rejected: true, synthetic: true, resource: { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'ReferenceGrant', metadata: { name, namespace: target.data.namespace } } } }
        nodes.push(denied); nodeMap.set(deniedId, denied)
      }
      addEdge(source.id, deniedId, 'no matching grant', 'unresolved'); addEdge(deniedId, target.id, 'denied', 'unresolved')
    }
  }

  for (const slice of resources.endpointslice ?? []) {
    const namespace = slice.metadata.namespace
    const serviceName = slice.metadata.labels?.['kubernetes.io/service-name']
    const sliceNode = nodeMap.get(nodeId('endpointslice', namespace, slice.metadata.name))
    if (serviceName && sliceNode) {
      const serviceRef: Reference = { kind: 'service', name: serviceName, namespace, label: 'endpoints' }
      const serviceNode = ensureTarget(serviceRef, sliceNode)
      addEdge(serviceNode.id, sliceNode.id, 'endpoints', serviceNode.data.unresolved ? 'unresolved' : 'resolved')
    }
    const sliceDocument = slice as K8sResource & { endpoints?: Record<string, unknown>[] }
    ;(sliceDocument.endpoints ?? slice.spec?.endpoints ?? []).forEach((endpoint: Record<string, unknown>, index: number) => {
      if (!sliceNode) return
      const addresses = Array.isArray(endpoint.addresses) ? endpoint.addresses.join(', ') : `endpoint-${index + 1}`
      const backendId = nodeId('backend', namespace, `${slice.metadata.name}:${addresses}`)
      const backend: TopoNode = {
        id: backendId,
        data: {
          kind: 'backend', name: addresses, namespace, layer: layerFor('backend'), synthetic: true,
          unhealthy: (endpoint.conditions as Record<string, unknown> | undefined)?.ready === false,
          resource: { apiVersion: 'discovery.k8s.io/v1', kind: 'Backend', metadata: { name: addresses, namespace }, status: endpoint.conditions },
        },
      }
      nodes.push(backend)
      nodeMap.set(backendId, backend)
      addEdge(sliceNode.id, backendId, 'address', 'resolved')
    })
  }

  if (namespaceFilter === null) {
    return { nodes, edges, namespaces: [...namespaces].sort() }
  }
  const visible = new Set(nodes.filter((node) => node.data.namespace === namespaceFilter).map((node) => node.id))
  let changed = true
  while (changed) {
    changed = false
    edges.forEach((edge) => {
      if (visible.has(edge.source) && !visible.has(edge.target)) { visible.add(edge.target); changed = true }
      if (visible.has(edge.target) && nodeMap.get(edge.source)?.data.namespace === undefined && !visible.has(edge.source)) {
        visible.add(edge.source); changed = true
      }
    })
  }
  return {
    nodes: nodes.filter((node) => visible.has(node.id)),
    edges: edges.filter((edge) => visible.has(edge.source) && visible.has(edge.target)),
    namespaces: [...namespaces].sort(),
  }
}

const QUERY_OPTIONS = { staleTime: 30_000, retry: 1 } as const

export function useTopologyData(namespaceFilter: string | null): TopologyData {
  const { controllerId } = useParams<{ controllerId?: string }>()
  const cid = controllerId ?? ''
  const results = useQueries({
    queries: TOPOLOGY_KINDS.map((kind) => ({
      queryKey: ['topology', kind, cid],
      queryFn: async () => {
        if (RESTRICTED_KINDS.has(kind)) {
          const response = await resourceApi.listKeys(kind, { silent: true })
          return (response.data ?? []).map((key) => ({ ...key })) as K8sResource[]
        }
        const response = CLUSTER_KINDS.has(kind)
          ? await clusterResourceApi.listAll<K8sResource>(kind, { silent: true })
          : await resourceApi.listAll<K8sResource>(kind, { silent: true })
        return response.data ?? []
      },
      ...QUERY_OPTIONS,
    })),
  })
  const isLoading = results.every((result) => result.isLoading)
  const resourceData = useMemo(() => Object.fromEntries(
    TOPOLOGY_KINDS.map((kind, index) => [kind, results[index]?.data ?? []]),
  ) as TopologyResources, results.map((result) => result.data)) // eslint-disable-line react-hooks/exhaustive-deps
  const partialErrors = results.flatMap((result, index) => result.isError ? [TOPOLOGY_KINDS[index]] : [])
  // ReferenceGrant validation is Controller process configuration, not an
  // EdgionGatewayConfig field. The resource API cannot reveal its value.
  const referenceGrantValidation = 'unknown' as const
  const graph = useMemo(
    () => buildTopologyGraph(resourceData, namespaceFilter, new Set(partialErrors), referenceGrantValidation),
    [resourceData, namespaceFilter, partialErrors.join('|'), referenceGrantValidation], // eslint-disable-line react-hooks/exhaustive-deps
  )
  const refetch = useCallback(() => { results.forEach((result) => result.refetch()) }, [results])
  return {
    ...graph,
    isLoading,
    isError: partialErrors.length === results.length,
    partialErrors,
    refetch,
  }
}
