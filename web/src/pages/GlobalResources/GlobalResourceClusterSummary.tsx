import { Alert, Card, Space, Tag, Typography } from 'antd'
import type {
  GlobalResourceClusterResult,
  GlobalResourceClusterResolution,
} from '@/api/globalResources'
import { useT } from '@/i18n'

interface Props {
  clusters: readonly (GlobalResourceClusterResult | GlobalResourceClusterResolution)[]
}

const stateColors: Record<GlobalResourceClusterResolution['state'], string> = {
  available: 'green',
  offline: 'red',
  ambiguous: 'orange',
  indeterminate: 'gold',
}

function isResult(
  cluster: GlobalResourceClusterResult | GlobalResourceClusterResolution,
): cluster is GlobalResourceClusterResult {
  return 'complete' in cluster
}

export default function GlobalResourceClusterSummary({ clusters }: Props) {
  const t = useT()
  const problemClusters = clusters.filter(
    (cluster) => cluster.state !== 'available' || (isResult(cluster) && !cluster.complete),
  )

  return (
    <Card size="small" title={t('globalResources.clusterSummary.title')}>
      <Space wrap>
        {clusters.map((cluster) => (
          <Tag key={cluster.cluster} color={stateColors[cluster.state]}>
            {cluster.cluster}: {t(`globalResources.clusterState.${cluster.state}`)}
            {isResult(cluster) && !cluster.complete
              ? ` · ${t('globalResources.clusterState.incomplete')}`
              : ''}
          </Tag>
        ))}
      </Space>

      {problemClusters.map((cluster) => (
        <Alert
          key={cluster.cluster}
          type={cluster.state === 'offline' ? 'error' : 'warning'}
          showIcon
          style={{ marginTop: 12 }}
          message={`${cluster.cluster}: ${t(`globalResources.clusterState.${cluster.state}`)}`}
          description={
            isResult(cluster) && cluster.errors.length > 0 ? (
              <Space direction="vertical" size={2}>
                {cluster.errors.map((error, index) => (
                  <Typography.Text
                    key={`${error.namespace}-${error.code}-${index}`}
                    data-testid="global-resource-cluster-error"
                  >
                    {error.namespace || t('globalResources.error.clusterScope')}
                    {' · '}
                    {t(`globalResources.errorCode.${error.code}`)}
                  </Typography.Text>
                ))}
              </Space>
            ) : (
              t('globalResources.clusterState.noAuthoritativeInventory')
            )
          }
        />
      ))}
    </Card>
  )
}
