import { useMemo, useState } from 'react'
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, Button, Card, Input, Select, Space, Spin } from 'antd'
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons'
import {
  globalResourcesApi,
  type GlobalResourceComparisonGroup,
} from '@/api/globalResources'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import type { GlobalResourceDescriptor } from './globalResourceDescriptors'
import GlobalResourceComparisonDrawer from './GlobalResourceComparisonDrawer'
import GlobalResourceComparisonTable from './GlobalResourceComparisonTable'

const PAGE_SIZE = 100

interface Props {
  descriptor: GlobalResourceDescriptor
}

export default function GlobalResourceInventoryPage({ descriptor }: Props) {
  const t = useT()
  const [clusters, setClusters] = useState<string[]>([])
  const [search, setSearch] = useState('')
  const [selectedGroup, setSelectedGroup] = useState<GlobalResourceComparisonGroup | null>(null)
  const queryClient = useQueryClient()
  const inventoryQueryKey = useMemo(() => [
    'global-resources',
    'inventory',
    descriptor.apiSlug,
    descriptor.configDataType,
    clusters,
  ] as const, [clusters, descriptor.apiSlug, descriptor.configDataType])
  const catalog = useQuery({
    queryKey: ['global-resources', 'catalog'],
    queryFn: globalResourcesApi.catalog,
    retry: false,
  })
  const inventory = useInfiniteQuery({
    queryKey: inventoryQueryKey,
    queryFn: ({ pageParam }) =>
      globalResourcesApi.list(descriptor.apiSlug, {
        clusters,
        configDataType: descriptor.configDataType,
        limit: PAGE_SIZE,
        continueToken: pageParam,
      }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.continueToken ?? undefined,
    retry: false,
  })

  const groups = useMemo(() => {
    const needle = search.trim().toLocaleLowerCase()
    const all = inventory.data?.pages.flatMap((page) => page.groups) ?? []
    if (!needle) return all
    return all.filter((group) =>
      `${group.key.namespace}/${group.key.name}`.toLocaleLowerCase().includes(needle),
    )
  }, [inventory.data, search])
  const refresh = async () => {
    await Promise.all([
      catalog.refetch(),
      queryClient.resetQueries({ queryKey: inventoryQueryKey, exact: true }),
    ])
  }

  return (
    <>
      <PageHeader
        title={descriptor.configDataType}
        subtitle={t('globalResources.inventory.subtitle')}
        actions={
          <Button
            icon={<ReloadOutlined />}
            onClick={() => void refresh()}
            loading={catalog.isFetching || inventory.isFetching}
          >
            {t('globalResources.action.refresh')}
          </Button>
        }
      />

      {(catalog.isError || inventory.isError) ? (
        <Alert
          type="error"
          showIcon
          style={{ marginBottom: 16 }}
          message={t('globalResources.error.loadFailed')}
          description={t('globalResources.error.loadFailedDescription')}
        />
      ) : null}

      <Card size="small" style={{ marginBottom: 16 }}>
        <Space wrap>
          <Select
            mode="multiple"
            allowClear
            style={{ minWidth: 320 }}
            value={clusters}
            onChange={setClusters}
            placeholder={t('globalResources.filter.allClusters')}
            aria-label={t('globalResources.filter.clusters')}
            options={(catalog.data?.clusters ?? []).map((cluster) => ({
              value: cluster.cluster,
              label: `${cluster.cluster} · ${t(`globalResources.clusterState.${cluster.state}`)}`,
            }))}
          />
          <Input
            allowClear
            prefix={<SearchOutlined />}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t('globalResources.filter.search')}
            aria-label={t('globalResources.filter.search')}
            style={{ width: 280 }}
          />
        </Space>
      </Card>

      {catalog.isLoading && inventory.isLoading ? (
        <Spin size="large" style={{ display: 'flex', justifyContent: 'center', minHeight: 200 }} />
      ) : (
        <Space direction="vertical" size={16} style={{ display: 'flex' }}>
          <GlobalResourceComparisonTable
            groups={groups}
            loading={inventory.isLoading}
            onOpen={setSelectedGroup}
          />
          {inventory.hasNextPage ? (
            <Button
              onClick={() => void inventory.fetchNextPage()}
              loading={inventory.isFetchingNextPage}
              block
            >
              {t('globalResources.action.loadMore')}
            </Button>
          ) : null}
        </Space>
      )}

      <GlobalResourceComparisonDrawer
        apiSlug={descriptor.apiSlug}
        group={selectedGroup}
        onClose={() => setSelectedGroup(null)}
      />
    </>
  )
}
