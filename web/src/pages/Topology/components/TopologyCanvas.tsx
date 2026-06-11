import { useRef, useEffect, useState, useCallback } from 'react'
import { Tag } from 'antd'
import type { TopoNode, TopoEdge } from '../hooks/useTopologyData'
import { NODE_TYPE_CONFIG } from './nodes/nodeStyles'
import type { K8sResource } from '@/types/gateway-api/common'

// ---------------------------------------------------------------------------

const GW_COL_W    = 152
const ROUTE_COL_W = 260
const SERVICE_COL_W = 192
const CARD_GAP = 10
const COL_PAD = 20

interface Line { key: string; x1: number; y1: number; x2: number; y2: number }

interface Props {
  nodes: TopoNode[]
  edges: TopoEdge[]
  plugins: K8sResource[]
  gateways: K8sResource[]
  onNodeClick: (nodeData: Record<string, any>) => void
}

interface PluginItem { name: string; phase: string }

const PHASE_LABEL: Record<string, string> = {
  req:    'request',
  resp:   'response',
  filter: 'resp-filter',
  body:   'body-filter',
}
const PHASE_COLOR: Record<string, string> = {
  req:    '#1890ff',
  resp:   '#52c41a',
  filter: '#fa8c16',
  body:   '#722ed1',
}

function pluginItems(p: K8sResource): PluginItem[] {
  const spec = p.spec ?? {}
  return [
    ...(spec.requestPlugins                    ?? []).map((x: any) => ({ name: x?.name ?? x?.type, phase: 'req'    })),
    ...(spec.upstreamResponsePlugins           ?? []).map((x: any) => ({ name: x?.name ?? x?.type, phase: 'resp'   })),
    ...(spec.upstreamResponseFilterPlugins     ?? []).map((x: any) => ({ name: x?.name ?? x?.type, phase: 'filter' })),
    ...(spec.upstreamResponseBodyFilterPlugins ?? []).map((x: any) => ({ name: x?.name ?? x?.type, phase: 'body'   })),
  ].filter(item => Boolean(item.name))
}

// Find all EdgionPlugins referenced by a route via extensionRef filters
function getRoutePlugins(routeResource: K8sResource, pluginMap: Map<string, K8sResource>): K8sResource[] {
  const ns = routeResource.metadata?.namespace ?? ''
  const seen = new Set<string>()
  const result: K8sResource[] = []
  for (const rule of routeResource.spec?.rules ?? []) {
    for (const filter of rule.filters ?? []) {
      if (
        filter.type === 'ExtensionRef' &&
        filter.extensionRef?.kind === 'EdgionPlugins'
      ) {
        const pluginNs   = filter.extensionRef.namespace ?? ns
        const pluginName = filter.extensionRef.name
        const key = `${pluginNs}/${pluginName}`
        if (!seen.has(key)) {
          seen.add(key)
          const p = pluginMap.get(key)
          if (p) result.push(p)
        }
      }
    }
  }
  return result
}

// ---------------------------------------------------------------------------

export default function TopologyCanvas({ nodes, edges, plugins, gateways, onNodeClick }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const routeRefs  = useRef(new Map<string, HTMLDivElement>())
  const serviceRefs = useRef(new Map<string, HTMLDivElement>())
  const [lines, setLines] = useState<Line[]>([])
  const [svgSize, setSvgSize] = useState({ w: 0, h: 0 })

  // Split nodes
  const routeNodes   = nodes.filter(n => n.data.kind !== 'service')
  const serviceNodes = nodes.filter(n => n.data.kind === 'service')

  // route → [serviceId]
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const routeToSvcs = new Map<string, string[]>()
  for (const edge of edges) {
    const list = routeToSvcs.get(edge.source) ?? []
    if (!list.includes(edge.target)) list.push(edge.target)
    routeToSvcs.set(edge.source, list)
  }

  // Ordered deduped services (first appearance across routes)
  const seen = new Set<string>()
  const orderedSvcIds: string[] = []
  for (const r of routeNodes) {
    for (const id of routeToSvcs.get(r.id) ?? []) {
      if (!seen.has(id)) { seen.add(id); orderedSvcIds.push(id) }
    }
  }
  const orderedSvcs = orderedSvcIds
    .map(id => serviceNodes.find(n => n.id === id))
    .filter(Boolean) as TopoNode[]

  // Plugin lookup: "namespace/name" → EdgionPlugins resource
  const pluginMap = new Map<string, K8sResource>()
  for (const p of plugins) {
    pluginMap.set(`${p.metadata.namespace}/${p.metadata.name}`, p)
  }

  // Gateway lookup: "namespace/name" → Gateway resource
  const gatewayMap = new Map<string, K8sResource>()
  for (const g of gateways) {
    gatewayMap.set(`${g.metadata.namespace}/${g.metadata.name}`, g)
  }

  // Measure and draw lines
  const measure = useCallback(() => {
    const c = containerRef.current
    if (!c) return
    const cR = c.getBoundingClientRect()
    const newLines: Line[] = []

    for (const route of routeNodes) {
      const rEl = routeRefs.current.get(route.id)
      if (!rEl) continue
      const rR = rEl.getBoundingClientRect()
      const x1 = rR.right  - cR.left
      const y1 = rR.top + rR.height / 2 - cR.top

      for (const svcId of routeToSvcs.get(route.id) ?? []) {
        const sEl = serviceRefs.current.get(svcId)
        if (!sEl) continue
        const sR = sEl.getBoundingClientRect()
        const x2 = sR.left - cR.left
        const y2 = sR.top + sR.height / 2 - cR.top
        newLines.push({ key: `${route.id}=>${svcId}`, x1, y1, x2, y2 })
      }
    }

    setLines(newLines)
    setSvgSize({ w: c.scrollWidth, h: c.scrollHeight })
  }, [routeNodes, routeToSvcs])

  useEffect(() => {
    const id = requestAnimationFrame(measure)
    return () => cancelAnimationFrame(id)
  }, [nodes, edges, measure])

  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [measure])

  return (
    <div
      ref={containerRef}
      style={{
        position: 'relative',
        display: 'flex',
        padding: COL_PAD,
        gap: 0,
        minHeight: '100%',
        boxSizing: 'border-box',
      }}
    >
      {/* SVG connection lines — absolute, covers full scrollable area */}
      <svg
        style={{
          position: 'absolute',
          top: 0, left: 0,
          width: svgSize.w || '100%',
          height: svgSize.h || '100%',
          pointerEvents: 'none',
          overflow: 'visible',
        }}
      >
        <defs>
          <marker id="topo-arrow" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto">
            <path d="M 0 0 L 7 3.5 L 0 7 Z" fill="#bfbfbf" />
          </marker>
        </defs>
        {lines.map(({ key, x1, y1, x2, y2 }) => {
          const cx = (x1 + x2) / 2
          return (
            <path
              key={key}
              d={`M ${x1} ${y1} C ${cx} ${y1} ${cx} ${y2} ${x2} ${y2}`}
              stroke="#bfbfbf"
              strokeWidth={1.5}
              fill="none"
              markerEnd="url(#topo-arrow)"
            />
          )
        })}
      </svg>

      {/* Left group: rows of [Gateway cell(s) → Route card] */}
      <div style={{ flexShrink: 0, display: 'flex', flexDirection: 'column', gap: CARD_GAP }}>
        {routeNodes.map(node => {
          const cfg         = NODE_TYPE_CONFIG[node.data.kind] ?? { color: '#8c8c8c' }
          const gwCfg       = NODE_TYPE_CONFIG['gateway']
          const routePlugins = getRoutePlugins(node.data.resource, pluginMap)
          const parentRefs: any[] = node.data.resource?.spec?.parentRefs ?? []

          return (
            <div key={node.id} style={{ display: 'flex', alignItems: 'flex-start', gap: 6 }}>

              {/* Gateway cells — one per parentRef */}
              <div style={{ width: GW_COL_W, flexShrink: 0, display: 'flex', flexDirection: 'column', gap: 4 }}>
                {parentRefs.length === 0 ? (
                  <div style={{ height: 36 }} />
                ) : parentRefs.map((ref: any, i: number) => {
                  const gwNs  = ref.namespace ?? node.data.namespace ?? ''
                  const gwRes = gatewayMap.get(`${gwNs}/${ref.name}`)
                  return (
                    <div
                      key={i}
                      onClick={() => onNodeClick({
                        kind: 'gateway',
                        name: ref.name,
                        namespace: gwNs,
                        resource: gwRes ?? { metadata: { name: ref.name, namespace: gwNs }, kind: 'Gateway' },
                        _matchedListener: ref.sectionName,
                      })}
                      style={{
                        border: `1px solid ${gwCfg.color}44`,
                        borderLeft: `3px solid ${gwCfg.color}`,
                        borderRadius: 5,
                        padding: '5px 8px',
                        background: 'var(--ec-color-bg-surface)',
                        boxShadow: 'var(--ec-shadow-sm)',
                        cursor: 'pointer',
                        transition: 'box-shadow 0.15s',
                      }}
                      onMouseEnter={e => (e.currentTarget.style.boxShadow = '0 2px 8px rgba(0,0,0,0.14)')}
                      onMouseLeave={e => (e.currentTarget.style.boxShadow = '0 1px 3px rgba(0,0,0,0.05)')}
                    >
                      <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--ec-color-text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {ref.name}
                      </div>
                      {ref.namespace && (
                        <div style={{ fontSize: 10, color: 'var(--ec-color-text-subtle)' }}>{ref.namespace}</div>
                      )}
                      {ref.sectionName && (
                        <Tag style={{ marginTop: 3, margin: '3px 0 0', fontSize: 10, padding: '0 4px', color: '#52c41a', borderColor: '#52c41a88', background: '#f6ffed' }}>
                          {ref.sectionName}
                        </Tag>
                      )}
                    </div>
                  )
                })}
              </div>

              {/* Arrow */}
              <div style={{ paddingTop: 10, color: 'var(--ec-color-text-subtle)', flexShrink: 0, fontSize: 13, userSelect: 'none' }}>→</div>

              {/* Route card */}
              <div
                ref={el => { el ? routeRefs.current.set(node.id, el) : routeRefs.current.delete(node.id) }}
                onClick={() => onNodeClick(node.data)}
                style={{
                  width: ROUTE_COL_W,
                  flexShrink: 0,
                  border: `1px solid ${cfg.color}55`,
                  borderLeft: `3px solid ${cfg.color}`,
                  borderRadius: 6,
                  padding: '8px 12px',
                  cursor: 'pointer',
                  background: 'var(--ec-color-bg-surface)',
                  boxShadow: 'var(--ec-shadow-sm)',
                  transition: 'box-shadow 0.15s',
                }}
                onMouseEnter={e => (e.currentTarget.style.boxShadow = '0 2px 8px rgba(0,0,0,0.14)')}
                onMouseLeave={e => (e.currentTarget.style.boxShadow = '0 1px 4px rgba(0,0,0,0.06)')}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <Tag style={{ margin: 0, fontSize: 11, color: cfg.color, borderColor: `${cfg.color}88`, background: `${cfg.color}18` }}>
                    {node.data.kind}
                  </Tag>
                  <span style={{ fontWeight: 500, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
                    {node.data.name}
                  </span>
                </div>

                {node.data.namespace && (
                  <div style={{ fontSize: 11, color: 'var(--ec-color-text-subtle)', marginTop: 2 }}>{node.data.namespace}</div>
                )}

                {routePlugins.map(plugin => {
                  const pItems = pluginItems(plugin)
                  return (
                    <div
                      key={plugin.metadata.name}
                      onClick={e => {
                        e.stopPropagation()
                        onNodeClick({ kind: 'edgionplugins', name: plugin.metadata.name, namespace: plugin.metadata.namespace, resource: plugin })
                      }}
                      style={{ marginTop: 8, border: '1px solid var(--ec-color-border)', borderRadius: 4, overflow: 'hidden', cursor: 'pointer' }}
                    >
                      <div style={{ padding: '3px 8px', background: 'var(--ec-color-bg-subtle)', borderBottom: '1px solid var(--ec-color-border)', display: 'flex', alignItems: 'center', gap: 5 }}>
                        <span style={{ width: 6, height: 6, borderRadius: 1, background: 'var(--ec-color-text-subtle)', flexShrink: 0 }} />
                        <span style={{ fontSize: 10, fontWeight: 600, color: 'var(--ec-color-text-muted)' }}>EdgionPlugins</span>
                        <span style={{ fontSize: 10, color: 'var(--ec-color-text-subtle)' }}>{plugin.metadata.name}</span>
                      </div>
                      <div style={{ padding: '4px 8px', display: 'flex', flexDirection: 'column', gap: 3, background: 'var(--ec-color-bg-subtle)' }}>
                        {pItems.map((item, i) => (
                          <div key={`${item.name}-${i}`} style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '2px 6px', background: 'var(--ec-color-bg-surface)', border: '1px solid var(--ec-color-border)', borderRadius: 3, fontSize: 11 }}>
                            <span style={{ width: 6, height: 6, borderRadius: '50%', background: PHASE_COLOR[item.phase] ?? '#8c8c8c', flexShrink: 0 }} />
                            <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: 'var(--ec-color-text)' }}>{item.name}</span>
                            <span style={{ fontSize: 9, color: 'var(--ec-color-text-subtle)', flexShrink: 0 }}>{PHASE_LABEL[item.phase] ?? item.phase}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                  )
                })}
              </div>

            </div>
          )
        })}
      </div>

      {/* Spacer — where SVG lines travel */}
      <div style={{ flex: 1, minWidth: 80 }} />

      {/* Right column: Service cards */}
      <div style={{ width: SERVICE_COL_W, flexShrink: 0, display: 'flex', flexDirection: 'column', gap: CARD_GAP }}>
        {orderedSvcs.map(node => {
          const cfg = NODE_TYPE_CONFIG['service']
          return (
            <div
              key={node.id}
              ref={el => { el ? serviceRefs.current.set(node.id, el) : serviceRefs.current.delete(node.id) }}
              onClick={() => onNodeClick(node.data)}
              style={{
                border: `1px solid ${cfg.color}55`,
                borderLeft: `3px solid ${cfg.color}`,
                borderRadius: 6,
                padding: '8px 12px',
                cursor: 'pointer',
                background: 'var(--ec-color-bg-surface)',
                boxShadow: 'var(--ec-shadow-sm)',
                transition: 'box-shadow 0.15s',
              }}
              onMouseEnter={e => (e.currentTarget.style.boxShadow = '0 2px 8px rgba(0,0,0,0.14)')}
              onMouseLeave={e => (e.currentTarget.style.boxShadow = '0 1px 4px rgba(0,0,0,0.06)')}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <Tag
                  style={{ margin: 0, fontSize: 11, color: cfg.color, borderColor: `${cfg.color}88`, background: `${cfg.color}18` }}
                >
                  service
                </Tag>
                <span style={{ fontWeight: 500, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
                  {node.data.name}
                </span>
              </div>
              {node.data.namespace && (
                <div style={{ fontSize: 11, color: 'var(--ec-color-text-subtle)', marginTop: 2 }}>{node.data.namespace}</div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
