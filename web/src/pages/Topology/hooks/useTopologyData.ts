import { useMemo, useCallback } from 'react'
import { useParams } from 'react-router-dom'
import { useQueries } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/types/gateway-api/common'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface TopoNode {
  id: string
  data: {
    kind: string
    name: string
    namespace?: string
    resource: K8sResource
    routeKind?: string   // 仅 route 节点有：'HTTPRoute' | 'GRPCRoute' | ...
    [key: string]: any
  }
}

export interface TopoEdge {
  id: string
  source: string
  target: string
  label?: string
  dashed?: boolean
}

interface TopologyData {
  nodes: TopoNode[]
  edges: TopoEdge[]
  namespaces: string[]
  plugins: K8sResource[]
  gateways: K8sResource[]
  isLoading: boolean
  isError: boolean
  refetch: () => void
}

interface BuildResult {
  nodes: TopoNode[]
  edges: TopoEdge[]
  namespaces: string[]
}

// ---------------------------------------------------------------------------
// Edge helper
// ---------------------------------------------------------------------------

function makeEdge(
  source: string,
  target: string,
  label?: string,
  dashed?: boolean,
): TopoEdge {
  return { id: `${source}->${target}`, source, target, label, dashed }
}

// ---------------------------------------------------------------------------
// Pure graph builder（保留所有原有的节点/边构建逻辑，只去掉 position 和 dagre）
// ---------------------------------------------------------------------------

export function buildTopologyGraph(
  httproutes: K8sResource[],
  grpcroutes: K8sResource[],
  tcproutes: K8sResource[],
  udproutes: K8sResource[],
  tlsroutes: K8sResource[],
  services: K8sResource[],
  namespaceFilter: string | null,
): BuildResult {
  const nodes: TopoNode[] = []
  const edgeSet = new Set<string>()
  const edges: TopoEdge[] = []

  function addEdge(edge: TopoEdge) {
    if (!edgeSet.has(edge.id)) {
      edgeSet.add(edge.id)
      edges.push(edge)
    }
  }

  // Route nodes
  const routeGroups: Array<[K8sResource[], string]> = [
    [httproutes, 'HTTPRoute'], [grpcroutes, 'GRPCRoute'],
    [tcproutes, 'TCPRoute'],   [udproutes, 'UDPRoute'],
    [tlsroutes, 'TLSRoute'],
  ]
  for (const [routeList, routeKind] of routeGroups) {
    for (const res of routeList) {
      const { name, namespace } = res.metadata
      const kindLower = routeKind.toLowerCase()
      nodes.push({
        id: `${kindLower}/${namespace}/${name}`,
        data: { kind: kindLower, routeKind, name, namespace, resource: res,
                hostnamesCount: res.spec?.hostnames?.length ?? 0,
                rulesCount: res.spec?.rules?.length ?? 0 },
      })
    }
  }

  // Service nodes
  for (const res of services) {
    const { name, namespace } = res.metadata
    nodes.push({
      id: `service/${namespace}/${name}`,
      data: { kind: 'service', name, namespace, resource: res,
              portsCount: res.spec?.ports?.length ?? 0 },
    })
  }

  const nodeIds = new Set(nodes.map((n) => n.id))

  // Edges: Route → Service
  for (const [routeList, routeKind] of routeGroups) {
    for (const res of routeList) {
      const { name, namespace } = res.metadata
      const kindLower = routeKind.toLowerCase()
      const routeId = `${kindLower}/${namespace}/${name}`
      for (const rule of res.spec?.rules ?? []) {
        for (const ref of rule.backendRefs ?? []) {
          if ((ref.kind ?? 'Service') === 'Service') {
            const target = `service/${ref.namespace ?? namespace}/${ref.name}`
            if (nodeIds.has(target))
              addEdge(makeEdge(routeId, target, ref.port ? `:${ref.port}` : undefined))
          }
        }
      }
    }
  }

  // Namespaces
  const nsSet = new Set<string>()
  for (const res of [...httproutes, ...grpcroutes, ...tcproutes,
                      ...udproutes, ...tlsroutes, ...services]) {
    if (res.metadata.namespace) nsSet.add(res.metadata.namespace)
  }
  const namespaces = Array.from(nsSet).sort()

  // Namespace filter
  let filteredNodes = nodes
  let filteredEdges = edges
  if (namespaceFilter !== null) {
    const visibleIds = new Set(nodes.filter((n) => n.data.namespace === namespaceFilter).map((n) => n.id))
    filteredNodes = nodes.filter((n) => visibleIds.has(n.id))
    filteredEdges = edges.filter((e) => visibleIds.has(e.source) && visibleIds.has(e.target))
  }

  return { nodes: filteredNodes, edges: filteredEdges, namespaces }
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

const QUERY_OPTIONS = { staleTime: 30000, retry: 1 } as const

export function useTopologyData(namespaceFilter: string | null): TopologyData {
  const { controllerId } = useParams<{ controllerId?: string }>()
  const cid = controllerId ?? ''
  const results = useQueries({
    queries: [
      { queryKey: ['topology', 'gateway',       cid], queryFn: () => resourceApi.listAll<K8sResource>('gateway').then((r) => r.data ?? []),       ...QUERY_OPTIONS },
      { queryKey: ['topology', 'httproute',     cid], queryFn: () => resourceApi.listAll<K8sResource>('httproute').then((r) => r.data ?? []),     ...QUERY_OPTIONS },
      { queryKey: ['topology', 'grpcroute',     cid], queryFn: () => resourceApi.listAll<K8sResource>('grpcroute').then((r) => r.data ?? []),     ...QUERY_OPTIONS },
      { queryKey: ['topology', 'tcproute',      cid], queryFn: () => resourceApi.listAll<K8sResource>('tcproute').then((r) => r.data ?? []),      ...QUERY_OPTIONS },
      { queryKey: ['topology', 'udproute',      cid], queryFn: () => resourceApi.listAll<K8sResource>('udproute').then((r) => r.data ?? []),      ...QUERY_OPTIONS },
      { queryKey: ['topology', 'tlsroute',      cid], queryFn: () => resourceApi.listAll<K8sResource>('tlsroute').then((r) => r.data ?? []),      ...QUERY_OPTIONS },
      { queryKey: ['topology', 'service',       cid], queryFn: () => resourceApi.listAll<K8sResource>('service').then((r) => r.data ?? []),       ...QUERY_OPTIONS },
      { queryKey: ['topology', 'edgionplugins', cid], queryFn: () => resourceApi.listAll<K8sResource>('edgionplugins').then((r) => r.data ?? []), ...QUERY_OPTIONS },
    ],
  })

  const isLoading = results.some((r) => r.isLoading)
  const isError = results.some((r) => r.isError)

  const graph = useMemo(() => {
    if (isLoading) return { nodes: [], edges: [], namespaces: [] }
    const [, http, grpc, tcp, udp, tls, svc] = results
    return buildTopologyGraph(
      http.data ?? [], grpc.data ?? [], tcp.data ?? [],
      udp.data ?? [], tls.data ?? [], svc.data ?? [], namespaceFilter,
    )
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...results.map((r) => r.data), namespaceFilter, isLoading])

  const gateways = results[0]?.data ?? []
  const plugins  = results[7]?.data ?? []

  const refetch = useCallback(() => { results.forEach((r) => r.refetch()) }, [results])

  return { nodes: graph.nodes, edges: graph.edges, namespaces: graph.namespaces, plugins, gateways, isLoading, isError, refetch }
}
