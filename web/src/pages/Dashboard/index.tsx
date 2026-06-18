import { useNavigate, useParams } from 'react-router-dom'
import { Button } from 'antd'
import {
  ClusterOutlined,
  AppstoreOutlined,
  SettingOutlined,
  ReloadOutlined,
  HeartOutlined,
} from '@ant-design/icons'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { resourceApi, clusterResourceApi } from '@/api/resources'
import { systemApi } from '@/api/client'
import type { K8sResource } from '@/api/types'
import type { Gateway } from '@/types/gateway-api/gateway'
import PageHeader from '@/components/PageHeader'
import { StatCard } from '@/components/widgets/StatCard'
import { ListCard } from '@/components/widgets/ListCard'
import { StatusDot } from '@/components/widgets/StatusDot'
import { useT } from '@/i18n'
import { getActiveControllerId } from '@/utils/proxy'

const Dashboard = () => {
  const t = useT()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { controllerId } = useParams<{ controllerId?: string }>()

  // Liveness + readiness are both derived from /api/v1/server-info (admin port).
  // /health and /ready now live on the controller's dedicated probe listener (:12100),
  // which the same-origin embedded frontend (served on the admin port :12101) cannot
  // reach — a same-origin GET /health would 404 in production. server-info is on the
  // admin port, so it is always reachable: a successful response means the backend is
  // alive, and its `ready` field carries readiness.
  const { data: serverInfo } = useQuery({
    queryKey: ['server-info', controllerId ?? ''],
    queryFn: systemApi.serverInfo,
    staleTime: 30 * 1000,
    retry: false,
  })

  const isHealthy = serverInfo?.success === true
  const isReady   = serverInfo?.data?.ready === true

  // Gateways — full list for count + recent gateways ListCard
  const { data: gatewaysResp } = useQuery({
    queryKey: ['gateways-dashboard', controllerId ?? ''],
    queryFn: () => resourceApi.listAll<Gateway>('gateway'),
    staleTime: 30 * 1000,
  })
  const gateways = gatewaysResp?.data ?? []

  // EdgionAcme resources — used to compute acmeExpiry stat.
  // The EdgionAcmeSpec does not carry a notAfter timestamp, so we fall back to
  // counting ALL EdgionAcme resources as a proxy for "certs to watch". If the
  // backend begins returning status.notAfter, replace this count with a filtered
  // count: items whose status.notAfter is within 30 days of today.
  const { data: acmeResp } = useQuery({
    queryKey: ['edgionacme-dashboard', controllerId ?? ''],
    queryFn: () => resourceApi.listAll<K8sResource>('edgionacme'),
    staleTime: 30 * 1000,
  })
  const acmeResources = acmeResp?.data ?? []

  // Compute acmeExpiry: count certs expiring within 30 days.
  // If status.notAfter is present use it; otherwise show total count as fallback.
  const now = Date.now()
  const thirtyDaysMs = 30 * 24 * 60 * 60 * 1000
  const acmeWithExpiry = acmeResources.filter((r) => {
    const notAfter: string | undefined = r.status?.notAfter
    if (!notAfter) return false
    const exp = new Date(notAfter).getTime()
    return exp - now <= thirtyDaysMs
  })
  // If no status.notAfter fields are populated fall back to total ACME resource count
  const acmeExpiryStat = acmeWithExpiry.length > 0 ? acmeWithExpiry.length : acmeResources.length

  // Derive controller health display value. isHealthy now reflects reachability of
  // /api/v1/server-info, so a single healthy/unhealthy split matches the header dot;
  // the old isReady-based middle branch became unreachable once both signals share
  // the same source.
  const controllerHealthValue = isHealthy ? t('status.healthy') : t('status.unhealthy')

  // Additional ops resource counts (preserved from original count grid)
  const gcQuery      = useQuery({ queryKey: ['count', 'gatewayclass',        controllerId ?? ''], queryFn: () => clusterResourceApi.listAll<K8sResource>('gatewayclass').then(r => r.count ?? r.data?.length ?? 0),        staleTime: 30000 })
  const pluginQuery  = useQuery({ queryKey: ['count', 'edgionplugins',        controllerId ?? ''], queryFn: () => resourceApi.listAll<K8sResource>('edgionplugins').then(r => r.count ?? r.data?.length ?? 0),              staleTime: 30000 })
  const streamQuery  = useQuery({ queryKey: ['count', 'edgionstreamplugins',  controllerId ?? ''], queryFn: () => resourceApi.listAll<K8sResource>('edgionstreamplugins').then(r => r.count ?? r.data?.length ?? 0),        staleTime: 30000 })
  const metaQuery    = useQuery({ queryKey: ['count', 'edgionconfigdata',       controllerId ?? ''], queryFn: () => resourceApi.listAll<K8sResource>('edgionconfigdata').then(r => r.count ?? r.data?.length ?? 0),             staleTime: 30000 })
  const gcfgQuery    = useQuery({ queryKey: ['count', 'edgiongatewayconfig',  controllerId ?? ''], queryFn: () => clusterResourceApi.listAll<K8sResource>('edgiongatewayconfig').then(r => r.count ?? r.data?.length ?? 0), staleTime: 30000 })
  const linksysQuery = useQuery({ queryKey: ['count', 'linksys',              controllerId ?? ''], queryFn: () => resourceApi.listAll<K8sResource>('linksys').then(r => r.count ?? r.data?.length ?? 0),                    staleTime: 30000 })
  const rgQuery      = useQuery({ queryKey: ['count', 'referencegrant',       controllerId ?? ''], queryFn: () => resourceApi.listAll<K8sResource>('referencegrant').then(r => r.count ?? r.data?.length ?? 0),              staleTime: 30000 })

  // Recent gateways list (top 6 by creationTimestamp desc)
  const recentGatewayRows = [...gateways]
    .sort((a, b) =>
      (b.metadata.creationTimestamp ?? '').localeCompare(a.metadata.creationTimestamp ?? '')
    )
    .slice(0, 6)
    .map((gw) => {
      const listenerCount = gw.spec?.listeners?.length ?? 0
      const gwClass = gw.spec?.gatewayClassName ?? '—'
      return {
        key: `${gw.metadata.namespace ?? ''}/${gw.metadata.name}`,
        tone: 'brand' as const,
        primary: gw.metadata.name,
        secondary: `${gwClass} · ${listenerCount} listener${listenerCount !== 1 ? 's' : ''}`,
        trailing: gw.metadata.namespace ?? '—',
        onClick: () => {
          const cid = getActiveControllerId()
          const prefix = cid ? `/controller/${cid.replace(/\//g, '~')}` : ''
          navigate(`${prefix}/infrastructure/gateways`)
        },
      }
    })

  const handleRefreshAll = () => {
    queryClient.invalidateQueries()
  }

  const goTo = (path: string) => {
    const cid = getActiveControllerId()
    const prefix = cid ? `/controller/${cid.replace(/\//g, '~')}` : ''
    navigate(`${prefix}${path}`)
  }

  // Secondary resource count tiles (preserving the original grid of ops resource counts)
  const secondaryResources = [
    { label: 'GatewayClass',       value: gcQuery.data      ?? 0, path: '/infrastructure/gatewayclasses', icon: <ClusterOutlined />  },
    { label: 'EdgionPlugins',       value: pluginQuery.data   ?? 0, path: '/plugins',                       icon: <AppstoreOutlined /> },
    { label: 'StreamPlugins',       value: streamQuery.data   ?? 0, path: '/plugins/stream',                icon: <AppstoreOutlined /> },
    { label: 'EdgionConfigData',     value: metaQuery.data     ?? 0, path: '/plugins/metadata',              icon: <AppstoreOutlined /> },
    { label: 'GatewayConfig',       value: gcfgQuery.data     ?? 0, path: '/system/config',                 icon: <SettingOutlined />  },
    { label: 'LinkSys',             value: linksysQuery.data  ?? 0, path: '/system/linksys',                icon: <SettingOutlined />  },
    { label: 'ReferenceGrant',      value: rgQuery.data       ?? 0, path: '',                               icon: <SettingOutlined />  },
  ]

  return (
    <div>
      <PageHeader
        title={t('nav.dashboard')}
        subtitle={t('page.subtitle.opsDashboard')}
        actions={
          <>
            <StatusDot tone={isHealthy ? 'success' : 'danger'} />
            <Button icon={<ReloadOutlined />} onClick={handleRefreshAll}>
              {t('dash.refreshAll')}
            </Button>
          </>
        }
      />

      {/* Primary stat cards: gateways, controllerHealth, acmeExpiry */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
          gap: 16,
          marginBottom: 24,
        }}
      >
        <StatCard
          label={t('dashboard.stat.gateways')}
          value={gateways.length}
          icon={<ClusterOutlined />}
        />
        <StatCard
          label={t('dashboard.stat.controllerHealth')}
          value={controllerHealthValue}
          icon={<HeartOutlined />}
        />
        {/* acmeExpiry: count of EdgionAcme resources expiring within 30 days.
            Falls back to total EdgionAcme count when status.notAfter is absent. */}
        <StatCard
          label={t('dashboard.stat.acmeExpiry')}
          value={acmeExpiryStat}
          icon={<SettingOutlined />}
        />
      </div>

      {/* Main content: recent gateways + system info */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))',
          gap: 16,
          marginBottom: 16,
        }}
      >
        {/* Recent gateways ListCard */}
        <ListCard
          title={t('dashboard.recent.gateways')}
          rows={recentGatewayRows}
          emptyText={t('dashboard.empty')}
        />

        {/* System info panel (preserved from original, restyled with token vars) */}
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
            {t('section.sysInfo')}
          </div>
          {[
            {
              label: t('dash.controllerStatus'),
              value: (
                <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <StatusDot tone={isHealthy ? 'success' : 'danger'} />
                  {isHealthy ? t('status.running') : t('status.unhealthy')}
                </span>
              ),
            },
            {
              label: t('dash.readyStatus'),
              value: (
                <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <StatusDot tone={isReady ? 'success' : 'warning'} />
                  {isReady ? t('status.ready') : t('status.notReady')}
                </span>
              ),
            },
            { label: t('dash.serverId'),  value: <code style={{ fontSize: 11 }}>{serverInfo?.data?.server_id || '—'}</code> },
            { label: t('dash.adminApi'),  value: <span>:12101</span> },
            { label: t('dash.grpc'),      value: <span>:12151</span> },
          ].map((row, idx) => (
            <div
              key={idx}
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                padding: '10px 16px',
                borderTop: '1px solid var(--ec-color-border)',
                fontSize: 'var(--ec-size-sm)',
              }}
            >
              <span style={{ color: 'var(--ec-color-text-muted)' }}>{row.label}</span>
              <span style={{ color: 'var(--ec-color-text)' }}>{row.value}</span>
            </div>
          ))}
        </div>
      </div>

      {/* Secondary resource count tiles — preserves original ops resource grid */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
          gap: 12,
        }}
      >
        {secondaryResources.map((item) => (
          <div
            key={item.label}
            onClick={() => item.path ? goTo(item.path) : undefined}
            style={{
              background: 'var(--ec-color-bg-subtle)',
              border: '1px solid var(--ec-color-border)',
              borderRadius: 'var(--ec-radius-sm)',
              padding: '10px 14px',
              cursor: item.path ? 'pointer' : 'default',
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
            }}
          >
            <span style={{ fontSize: 'var(--ec-size-sm)', color: 'var(--ec-color-text-muted)' }}>
              {item.label}
            </span>
            <span style={{ fontSize: 'var(--ec-size-md)', fontWeight: 600, color: 'var(--ec-color-text)' }}>
              {item.value}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

export default Dashboard
