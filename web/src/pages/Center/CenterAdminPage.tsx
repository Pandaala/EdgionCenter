import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Table, Button, Space, Tag, Badge, Modal, Typography, message } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { centerApi, type AdminControllerDto } from '@/api/center'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'
import { useCan } from '@/utils/permissions'

const { Text } = Typography

export default function CenterAdminPage() {
  const t = useT()
  const queryClient = useQueryClient()
  const canDelete = useCan('controllers:write')

  const { data, isLoading, refetch } = useQuery({
    queryKey: ['center-admin-controllers'],
    queryFn: centerApi.listAdminControllers,
    staleTime: 30000,
  })

  const controllers: AdminControllerDto[] = data?.data ?? []

  const deleteMutation = useMutation({
    mutationFn: (id: string) => centerApi.deleteAdminController(id),
    onSuccess: () => {
      message.success(t('center.admin.deleteControllerOk'))
      queryClient.invalidateQueries({ queryKey: ['center-admin-controllers'] })
    },
    onError: (e: any) => {
      message.error(t('center.admin.deleteControllerFail', { err: e.message }))
    },
  })

  const handleDelete = (record: AdminControllerDto) => {
    Modal.confirm({
      title: t('confirm.deleteTitle'),
      content: t('center.admin.deleteControllerConfirm', { id: record.controllerId }),
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate(record.controllerId),
    })
  }

  const columns = [
    {
      title: t('center.controllerId'),
      dataIndex: 'controllerId',
      key: 'controllerId',
      render: (v: string) => <Text strong>{v}</Text>,
    },
    {
      title: t('center.cluster'),
      dataIndex: 'cluster',
      key: 'cluster',
      render: (v: string) => <Tag>{v}</Tag>,
    },
    {
      title: t('center.status'),
      dataIndex: 'online',
      key: 'online',
      render: (v: boolean) => (
        v
          ? <Badge status="success" text={t('center.online')} />
          : <Badge status="error" text={t('center.offline')} />
      ),
    },
    {
      title: t('center.admin.lastSeen'),
      dataIndex: 'lastSeenAt',
      key: 'lastSeenAt',
      render: (v: number) =>
        v ? new Date(v * 1000).toLocaleString() : t('center.never'),
    },
    {
      title: t('center.admin.envTag'),
      key: 'envTag',
      render: (_: unknown, record: AdminControllerDto) => (
        <Space wrap size={4}>
          {record.env.map((e) => <Tag key={`env-${e}`} color="blue">{e}</Tag>)}
          {record.tag.map((tg) => <Tag key={`tag-${tg}`} color="purple">{tg}</Tag>)}
        </Space>
      ),
    },
    {
      title: t('col.actions'),
      key: 'actions',
      render: (_: unknown, record: AdminControllerDto) => (
        canDelete ? (
          <Button
            danger
            size="small"
            onClick={() => handleDelete(record)}
            loading={deleteMutation.isPending && deleteMutation.variables === record.controllerId}
          >
            {t('center.admin.deleteController')}
          </Button>
        ) : null
      ),
    },
  ]

  return (
    <div>
      <PageHeader
        title={t('center.admin.title')}
        actions={
          <>
            <Button icon={<ReloadOutlined />} onClick={() => refetch()}>
              {t('btn.refresh')}
            </Button>
          </>
        }
      />

      <Table
        dataSource={controllers}
        columns={columns}
        rowKey="controllerId"
        loading={isLoading}
        pagination={{
          pageSize: 20,
          showTotal: (n) => t('table.totalItems', { n }),
        }}
      />
    </div>
  )
}
