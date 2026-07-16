import { useNavigate, useParams } from 'react-router-dom'
import { Button } from 'antd'
import {
  ApiOutlined,
  GlobalOutlined,
  DatabaseOutlined,
  SafetyOutlined,
  ReloadOutlined,
} from '@ant-design/icons'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import { systemApi } from '@/api/client'
import type { K8sResource } from '@/api/types'
import PageHeader from '@/components/PageHeader'
import { StatCard } from '@/components/widgets/StatCard'
import { ListCard } from '@/components/widgets/ListCard'
import { StatusDot } from '@/components/widgets/StatusDot'
import { useT } from '@/i18n'
import { getActiveControllerId } from '@/utils/proxy'

// Route kinds that count toward the "active routes" stat
const ROUTE_KINDS = ['httproute', 'grpcroute', 'tcproute', 'udproute', 'tlsroute'] as const

interface RouteResource extends K8sResource {
  spec?: {
    hostnames?: string[]
    rules?: Array<{
      backendRefs?: Array<{ name: string; namespace?: string }>
    }>
  }
}

const UserDashboard = () => {
  const t = useT()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  // Liveness is derived from /api/v1/server-info (admin port), not the same-origin
  // /health: /health moved to the controller's dedicated probe listener (:12100),
  // unreachable from the same-origin embedded frontend served on the admin port :12101.
  const { data: serverInfo } = useQuery({
    queryKey: ['server-info', controllerId ?? ''],
    queryFn: systemApi.serverInfo,
    staleTime: 30 * 1000,
    retry: false,
  })
  const isHealthy = serverInfo?.success === true

  // Fetch HTTPRoutes for the route-derived stats (hostnames, backends, recent list).
  // The original file showed per-kind counts via a lightweight count query; here we
  // fetch HTTPRoutes in full so we can derive hostnames and backend refs, then add
  // the remaining route kinds as count-only queries to build the total.
  const { data: httpRoutesResp } = useQuery({
    queryKey: ['httproutes-dashboard', controllerId ?? ''],
    queryFn: () => resourceApi.listAll<RouteResource>('httproute'),
    staleTime: 30 * 1000,
  })
  const httpRoutes = httpRoutesResp?.data ?? []

  // Per-kind counts for the other route types (grpc/tcp/udp/tls)
  const grpcQuery  = useQuery({ queryKey: ['count', 'grpcroute', controllerId ?? ''],  queryFn: () => resourceApi.listAll<K8sResource>('grpcroute').then(r => r.count ?? r.data?.length ?? 0),  staleTime: 30000 })
  const tcpQuery   = useQuery({ queryKey: ['count', 'tcproute',  controllerId ?? ''],  queryFn: () => resourceApi.listAll<K8sResource>('tcproute').then(r => r.count ?? r.data?.length ?? 0),   staleTime: 30000 })
  const udpQuery   = useQuery({ queryKey: ['count', 'udproute',  controllerId ?? ''],  queryFn: () => resourceApi.listAll<K8sResource>('udproute').then(r => r.count ?? r.data?.length ?? 0),   staleTime: 30000 })
  const tlsQuery   = useQuery({ queryKey: ['count', 'tlsroute',  controllerId ?? ''],  queryFn: () => resourceApi.listAll<K8sResource>('tlsroute').then(r => r.count ?? r.data?.length ?? 0),   staleTime: 30000 })

  // Total active routes = all route kinds combined
  const totalRoutes =
    httpRoutes.length +
    (grpcQuery.data ?? 0) +
    (tcpQuery.data  ?? 0) +
    (udpQuery.data  ?? 0) +
    (tlsQuery.data  ?? 0)

  // Derive unique hostnames and backend refs from HTTPRoutes
  const hostnames = new Set<string>()
  const backends  = new Set<string>()
  httpRoutes.forEach((r) => {
    r.spec?.hostnames?.forEach((h) => hostnames.add(h))
    r.spec?.rules?.forEach((rule) =>
      rule.backendRefs?.forEach((b) =>
        backends.add(`${b.namespace ?? r.metadata.namespace ?? ''}/${b.name}`)
      )
    )
  })

  // Recent routes: top 6 HTTPRoutes sorted by creationTimestamp descending
  const recentRows = [...httpRoutes]
    .sort((a, b) =>
      (b.metadata.creationTimestamp ?? '').localeCompare(a.metadata.creationTimestamp ?? '')
    )
    .slice(0, 6)
    .map((r) => {
      const hostname  = r.spec?.hostnames?.[0] ?? '—'
      const backendName = r.spec?.rules?.[0]?.backendRefs?.[0]?.name ?? '—'
      return {
        key: `${r.metadata.namespace ?? ''}/${r.metadata.name}`,
        tone: 'success' as const,
        primary: r.metadata.name,
        secondary: `${hostname} → ${backendName}`,
        trailing: r.metadata.namespace ?? '—',
        onClick: () => {
          const cid = getActiveControllerId()
          const prefix = cid ? `/controller/${cid.replace(/\//g, '~')}` : ''
          navigate(`${prefix}/routes/http`)
        },
      }
    })

  const handleRefreshAll = () => {
    queryClient.invalidateQueries()
  }

  // Quick-navigation helper
  const goTo = (path: string) => {
    const cid = getActiveControllerId()
    const prefix = cid ? `/controller/${cid.replace(/\//g, '~')}` : ''
    navigate(`${prefix}${path}`)
  }

  // Quick links (preserved from original, adapted to new visual language)
  const quickLinks: Array<{ label: string; path: string; tone: 'success' | 'warning' | 'danger' | 'muted' | 'brand' }> = [
    { label: 'HTTPRoute',        path: '/routes/http',          tone: 'success' },
    { label: 'GRPCRoute',        path: '/routes/grpc',          tone: 'brand'   },
    { label: 'Services',         path: '/services/list',        tone: 'success' },
    { label: 'EdgionTls',        path: '/security/tls',         tone: 'warning' },
    { label: 'BackendTLS Policy',path: '/security/backendtls',  tone: 'danger'  },
  ]

  return (
    <div>
      <PageHeader
        title={t('nav.dashboard')}
        subtitle={t('page.subtitle.userDashboard')}
        actions={
          <>
            <StatusDot tone={isHealthy ? 'success' : 'danger'} />
            <Button data-testid="user-refresh" icon={<ReloadOutlined />} onClick={handleRefreshAll}>
              {t('dash.refreshAll')}
            </Button>
          </>
        }
      />

      {/* Stat cards — activeRoutes, hostnames, backends */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
          gap: 16,
          marginBottom: 24,
        }}
      >
        <StatCard
          label={t('dashboard.stat.activeRoutes')}
          value={totalRoutes}
          icon={<ApiOutlined />}
        />
        <StatCard
          label={t('dashboard.stat.hostnames')}
          value={hostnames.size}
          icon={<GlobalOutlined />}
        />
        <StatCard
          label={t('dashboard.stat.backends')}
          value={backends.size}
          icon={<DatabaseOutlined />}
        />
      </div>

      {/* Main content grid: recent routes + quick links */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))',
          gap: 16,
        }}
      >
        {/* Recent HTTPRoutes list */}
        <ListCard
          title={t('dashboard.recent.routes')}
          rows={recentRows}
          emptyText={t('dashboard.empty')}
        />

        {/* Quick links — preserved from original dashboard, restyled */}
        <div
          style={{
            background: 'var(--ec-color-bg-surface)',
            border: '1px solid var(--ec-color-border)',
            borderRadius: 'var(--ec-radius-md)',
            boxShadow: 'var(--ec-shadow-sm)',
            overflow: 'hidden',
          }}
        >
          <div
            style={{
              padding: '12px 16px',
              fontSize: 'var(--ec-size-md)',
              fontWeight: 600,
              color: 'var(--ec-color-text)',
              borderBottom: '1px solid var(--ec-color-border)',
            }}
          >
            {t('section.quickLinks')}
          </div>
          {quickLinks.map((link) => (
            <div
              key={link.path}
              onClick={() => goTo(link.path)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 12,
                padding: '12px 16px',
                borderTop: '1px solid var(--ec-color-border)',
                cursor: 'pointer',
              }}
            >
              <StatusDot tone={link.tone} />
              <span
                style={{
                  fontSize: 'var(--ec-size-base)',
                  color: 'var(--ec-color-text)',
                }}
              >
                {link.label}
              </span>
              <SafetyOutlined
                style={{
                  marginLeft: 'auto',
                  color: 'var(--ec-color-text-muted)',
                  fontSize: 12,
                  display: ['service', 'services'].some(s => link.path.includes(s)) ? 'none' : undefined,
                }}
              />
            </div>
          ))}
        </div>
      </div>

      {/* Route-kind breakdown row (original showed per-kind counts — kept as a secondary stat row) */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(140px, 1fr))',
          gap: 12,
          marginTop: 16,
        }}
      >
        {ROUTE_KINDS.map((kind) => {
          const label = kind.replace('route', 'Route').replace(/^./, (c) => c.toUpperCase())
          const value =
            kind === 'httproute' ? httpRoutes.length
            : kind === 'grpcroute' ? (grpcQuery.data ?? 0)
            : kind === 'tcproute'  ? (tcpQuery.data  ?? 0)
            : kind === 'udproute'  ? (udpQuery.data  ?? 0)
            : (tlsQuery.data  ?? 0)
          return (
            <div
              key={kind}
              onClick={() => goTo(`/routes/${kind.replace('route', '')}`)}
              style={{
                background: 'var(--ec-color-bg-subtle)',
                border: '1px solid var(--ec-color-border)',
                borderRadius: 'var(--ec-radius-sm)',
                padding: '10px 14px',
                cursor: 'pointer',
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
              }}
            >
              <span style={{ fontSize: 'var(--ec-size-sm)', color: 'var(--ec-color-text-muted)' }}>
                {label}
              </span>
              <span style={{ fontSize: 'var(--ec-size-md)', fontWeight: 600, color: 'var(--ec-color-text)' }}>
                {value}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}

export default UserDashboard
