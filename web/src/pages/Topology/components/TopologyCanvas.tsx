import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Badge, Tag, Tooltip } from 'antd'
import type { TopoEdge, TopoNode } from '../hooks/useTopologyData'
import { NODE_TYPE_CONFIG } from './nodes/nodeStyles'

export const TOPOLOGY_EDGE_COLORS: Record<TopoEdge['state'], string> = {
  resolved: '#91a3b0', conflict: '#fa8c16', unresolved: '#ff4d4f', unavailable: '#8c8c8c', unknown: '#722ed1',
}

const CARD_WIDTH = 210
const COLUMN_GAP = 92
const ROW_GAP = 12
const PADDING = 20

interface Props {
  nodes: TopoNode[]
  edges: TopoEdge[]
  onNodeClick: (nodeData: TopoNode['data']) => void
}

interface Line {
  id: string
  x1: number
  y1: number
  x2: number
  y2: number
  edge: TopoEdge
}

const LAYER_LABELS = ['Gateway', 'Route', 'Service & Policy', 'Plugins', 'Dependencies', 'Endpoints & Data']

function configFor(kind: string) {
  return NODE_TYPE_CONFIG[kind] ?? { color: '#8c8c8c', bgColor: '#fafafa', label: kind }
}

function statusBadge(node: TopoNode) {
  if (node.data.unresolved) return <Tag color="red">unresolved</Tag>
  if (node.data.unavailable) return <Tag>unavailable</Tag>
  if (node.data.conflict) return <Tag color="orange">conflict</Tag>
  if (node.data.rejected) return <Tag color="red">rejected</Tag>
  if (node.data.unhealthy) return <Tag color="orange">not ready</Tag>
  return null
}

export default function TopologyCanvas({ nodes, edges, onNodeClick }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const cardRefs = useRef(new Map<string, HTMLDivElement>())
  const [lines, setLines] = useState<Line[]>([])
  const [size, setSize] = useState({ width: 0, height: 0 })
  const columns = useMemo(() => Array.from({ length: 6 }, (_, layer) => (
    nodes.filter((node) => node.data.layer === layer).sort((a, b) => (
      `${a.data.namespace ?? ''}/${a.data.kind}/${a.data.name}`.localeCompare(
        `${b.data.namespace ?? ''}/${b.data.kind}/${b.data.name}`,
      )
    ))
  )), [nodes])

  const measure = useCallback(() => {
    const container = containerRef.current
    if (!container) return
    const containerRect = container.getBoundingClientRect()
    const nextLines: Line[] = []
    edges.forEach((edge) => {
      const source = cardRefs.current.get(edge.source)
      const target = cardRefs.current.get(edge.target)
      if (!source || !target) return
      const sourceRect = source.getBoundingClientRect()
      const targetRect = target.getBoundingClientRect()
      const leftToRight = sourceRect.left <= targetRect.left
      nextLines.push({
        id: edge.id,
        x1: (leftToRight ? sourceRect.right : sourceRect.left) - containerRect.left,
        y1: sourceRect.top + sourceRect.height / 2 - containerRect.top,
        x2: (leftToRight ? targetRect.left : targetRect.right) - containerRect.left,
        y2: targetRect.top + targetRect.height / 2 - containerRect.top,
        edge,
      })
    })
    setLines(nextLines)
    setSize({ width: container.scrollWidth, height: container.scrollHeight })
  }, [edges])

  useEffect(() => {
    const frame = requestAnimationFrame(measure)
    return () => cancelAnimationFrame(frame)
  }, [nodes, edges, measure])

  useEffect(() => {
    const observer = new ResizeObserver(measure)
    if (containerRef.current) observer.observe(containerRef.current)
    return () => observer.disconnect()
  }, [measure])

  return (
    <div
      ref={containerRef}
      data-testid="topology-canvas"
      style={{
        position: 'relative', display: 'flex', gap: COLUMN_GAP, padding: PADDING,
        minWidth: 6 * CARD_WIDTH + 5 * COLUMN_GAP + 2 * PADDING,
        minHeight: '100%', alignItems: 'flex-start', boxSizing: 'border-box',
      }}
    >
      <svg
        aria-hidden
        style={{ position: 'absolute', inset: 0, width: size.width || '100%', height: size.height || '100%', pointerEvents: 'none' }}
      >
        <defs>
          {(['resolved', 'unresolved', 'conflict', 'unavailable', 'unknown'] as const).map((state) => (
            <marker key={state} id={`topology-arrow-${state}`} markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto">
              <path d="M 0 0 L 7 3.5 L 0 7 Z" fill={TOPOLOGY_EDGE_COLORS[state]} />
            </marker>
          ))}
        </defs>
        {lines.map(({ id, x1, y1, x2, y2, edge }) => {
          const middle = (x1 + x2) / 2
          const color = TOPOLOGY_EDGE_COLORS[edge.state]
          return (
            <g key={id}>
              <path
                d={`M ${x1} ${y1} C ${middle} ${y1} ${middle} ${y2} ${x2} ${y2}`}
                stroke={color}
                strokeWidth={edge.state === 'resolved' ? 1.4 : 2}
                strokeDasharray={edge.dashed ? '5 4' : undefined}
                fill="none"
                markerEnd={`url(#topology-arrow-${edge.state})`}
              />
              {edge.label && (
                <text x={middle} y={(y1 + y2) / 2 - 4} textAnchor="middle" fill={color} fontSize="10">
                  {edge.label}
                </text>
              )}
            </g>
          )
        })}
      </svg>

      {columns.map((column, layer) => (
        <div key={layer} style={{ width: CARD_WIDTH, flexShrink: 0, position: 'relative' }}>
          <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--ec-color-text-muted)', marginBottom: 8, textTransform: 'uppercase' }}>
            {LAYER_LABELS[layer]} <Badge count={column.length} showZero color="#8c8c8c" />
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: ROW_GAP }}>
            {column.map((node) => {
              const config = configFor(node.data.kind)
              const status = statusBadge(node)
              return (
                <Tooltip
                  key={node.id}
                  title={node.data.unresolved ? 'Referenced resource was not returned by this Controller' : undefined}
                  placement="top"
                >
                  <div
                    ref={(element) => { element ? cardRefs.current.set(node.id, element) : cardRefs.current.delete(node.id) }}
                    data-testid="topology-node"
                    data-node-testid={`topology-node-${node.data.kind}-${node.data.name}`}
                    onClick={() => onNodeClick(node.data)}
                    style={{
                      border: `1px ${node.data.unresolved ? 'dashed' : 'solid'} ${node.data.unresolved ? '#ff4d4f' : `${config.color}66`}`,
                      borderLeft: `4px solid ${node.data.unresolved ? '#ff4d4f' : config.color}`,
                      borderRadius: 6, padding: '8px 10px', cursor: 'pointer',
                      background: node.data.unresolved ? '#fff2f0' : 'var(--ec-color-bg-surface)',
                      boxShadow: 'var(--ec-shadow-sm)', position: 'relative', zIndex: 1,
                    }}
                  >
                    <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                      <Tag style={{ margin: 0, fontSize: 10, color: config.color, borderColor: `${config.color}88`, background: config.bgColor }}>
                        {node.data.kind}
                      </Tag>
                      <span style={{ fontSize: 12, fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
                        {node.data.name}
                      </span>
                    </div>
                    {node.data.namespace && <div style={{ fontSize: 10, color: 'var(--ec-color-text-subtle)', marginTop: 3 }}>{node.data.namespace}</div>}
                    {status && <div style={{ marginTop: 5 }}>{status}</div>}
                  </div>
                </Tooltip>
              )
            })}
          </div>
        </div>
      ))}
    </div>
  )
}
