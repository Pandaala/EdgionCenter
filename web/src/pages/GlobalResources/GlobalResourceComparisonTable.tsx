import { Button, Table, Tag } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import type { GlobalResourceComparisonGroup } from '@/api/globalResources'
import { useT } from '@/i18n'
import { getGlobalResourceConsistency } from './globalResourceConsistency'

interface Props {
  groups: readonly GlobalResourceComparisonGroup[]
  loading?: boolean
  onOpen: (group: GlobalResourceComparisonGroup) => void
}

export default function GlobalResourceComparisonTable({
  groups,
  loading = false,
  onOpen,
}: Props) {
  const t = useT()
  const columns: ColumnsType<GlobalResourceComparisonGroup> = [
    {
      title: t('globalResources.column.namespace'),
      dataIndex: ['key', 'namespace'],
    },
    {
      title: t('globalResources.column.name'),
      dataIndex: ['key', 'name'],
    },
    {
      title: t('globalResources.column.members'),
      render: (_, group) => group.members.length,
    },
    {
      title: t('globalResources.column.consistency'),
      render: (_, group) => {
        const consistency = getGlobalResourceConsistency(group)
        const color = consistency === 'consistent'
          ? 'success'
          : consistency === 'inconsistent'
            ? 'error'
            : 'default'
        return (
          <Tag color={color} data-testid={`global-resource-consistency-${consistency}`}>
            {t(`globalResources.consistency.${consistency}`)}
          </Tag>
        )
      },
    },
    {
      title: t('globalResources.column.clusters'),
      render: (_, group) => (
        <>
          {[...new Set(group.members.map((member) => member.cluster))].map((cluster) => (
            <Tag key={cluster}>{cluster}</Tag>
          ))}
        </>
      ),
    },
    {
      title: t('globalResources.column.actions'),
      width: 140,
      render: (_, group) => (
        <Button size="small" onClick={() => onOpen(group)}>
          {t('globalResources.action.compare')}
        </Button>
      ),
    },
  ]

  return (
    <Table
      rowKey={(group) => `${group.key.kind}/${group.key.namespace}/${group.key.name}`}
      columns={columns}
      dataSource={[...groups]}
      loading={loading}
      pagination={false}
      locale={{ emptyText: t('globalResources.empty') }}
    />
  )
}
