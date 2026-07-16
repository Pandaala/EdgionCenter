import { useEffect, useMemo, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Table, Button, Space, Tag, Modal, Form, Input, Checkbox, Card, Typography, Empty, message } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { rolesApi, type RoleDto } from '@/api/roles'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'

const { Title, Text } = Typography
const ROLES_KEY = ['center-admin-roles']

export default function RoleManagementPage() {
  const t = useT()
  const queryClient = useQueryClient()

  const [createForm] = Form.useForm()
  const [createOpen, setCreateOpen] = useState(false)
  const [selectedId, setSelectedId] = useState<number | null>(null)
  // Working copy of the selected role's permission keys, edited by the matrix.
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set())

  const { data: rolesData, isLoading, refetch } = useQuery({
    queryKey: ROLES_KEY,
    queryFn: rolesApi.list,
    staleTime: 30000,
  })
  const roles: RoleDto[] = useMemo(() => rolesData?.data ?? [], [rolesData?.data])

  const { data: catalogData } = useQuery({
    queryKey: ['center-permission-catalog'],
    queryFn: rolesApi.permissionCatalog,
    staleTime: Infinity,
  })
  const catalog = useMemo(() => catalogData?.data ?? [], [catalogData?.data])

  // Flat catalog order — drives deterministic key ordering when saving.
  const catalogOrder = useMemo(() => catalog.flatMap((g) => g.keys), [catalog])

  const selectedRole = roles.find((r) => r.id === selectedId) ?? null

  // Selection lifecycle, driven by the role list (create/delete/refetch):
  //  - no roles  → clear the selection and the editable matrix;
  //  - nothing selected → atomically select and seed the first role;
  //  - selected role deleted → select and seed the new first role.
  // It deliberately does NOT re-seed selectedKeys for a still-present selection,
  // so a background refetch of OTHER roles can't clobber unsaved toggles.
  useEffect(() => {
    if (roles.length === 0) {
      setSelectedId(null)
      setSelectedKeys(new Set())
      return
    }
    if (selectedId == null) {
      setSelectedId(roles[0].id)
      setSelectedKeys(new Set(roles[0].permissionKeys))
      return
    }
    if (!roles.some((r) => r.id === selectedId)) {
      setSelectedId(roles[0].id)
      setSelectedKeys(new Set(roles[0].permissionKeys))
    }
  }, [roles, selectedId])

  const selectRole = (role: RoleDto) => {
    setSelectedId(role.id)
    setSelectedKeys(new Set(role.permissionKeys))
  }

  const toggleKey = (key: string, checked: boolean) => {
    setSelectedKeys((prev) => {
      const next = new Set(prev)
      if (checked) next.add(key)
      else next.delete(key)
      return next
    })
  }

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ROLES_KEY })

  const createMutation = useMutation({
    mutationFn: rolesApi.create,
    onSuccess: () => {
      message.success(t('roles.msg.createOk'))
      setCreateOpen(false)
      createForm.resetFields()
      invalidate()
    },
    onError: (e: any) => message.error(t('roles.msg.createFail', { err: e.message })),
  })

  const saveMutation = useMutation({
    mutationFn: ({ id, keys }: { id: number; keys: string[] }) => rolesApi.setPermissions(id, keys),
    onSuccess: () => {
      message.success(t('roles.msg.saveOk'))
      invalidate()
    },
    onError: (e: any) => message.error(t('roles.msg.saveFail', { err: e.message })),
  })

  const deleteMutation = useMutation({
    mutationFn: (id: number) => rolesApi.remove(id),
    onSuccess: () => {
      message.success(t('roles.msg.deleteOk'))
      invalidate()
    },
    onError: (e: any) => message.error(t('roles.msg.deleteFail', { err: e.message })),
  })

  const handleCreate = async () => {
    const values = await createForm.validateFields()
    createMutation.mutate({ name: values.name, description: values.description || undefined })
  }

  const handleSave = () => {
    if (!selectedRole) return
    // Preserve any held key the catalog doesn't manage (version skew /
    // deprecated-but-assigned), then apply checkbox state over catalog keys
    // in catalog order for a stable, predictable payload.
    const catalogKeys = new Set(catalogOrder)
    const preserved = selectedRole.permissionKeys.filter((k) => !catalogKeys.has(k))
    const checked = catalogOrder.filter((k) => selectedKeys.has(k))
    const keys = Array.from(new Set([...preserved, ...checked]))
    saveMutation.mutate({ id: selectedRole.id, keys })
  }

  const handleDelete = (record: RoleDto) => {
    Modal.confirm({
      title: t('confirm.deleteTitle'),
      content: t('roles.confirm.delete', { name: record.name }),
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      okButtonProps: { 'data-testid': 'role-confirm' },
      cancelButtonProps: { 'data-testid': 'role-cancel' },
      onOk: () => deleteMutation.mutate(record.id),
    })
  }

  const columns = [
    { title: t('roles.col.name'), dataIndex: 'name', key: 'name' },
    { title: t('roles.col.description'), dataIndex: 'description', key: 'description', render: (v: string) => v || '-' },
    {
      title: t('roles.col.permissions'),
      dataIndex: 'permissionKeys',
      key: 'permissionKeys',
      render: (v: string[]) => <Tag>{v.length}</Tag>,
    },
    {
      title: t('col.actions'),
      key: 'actions',
      render: (_: unknown, record: RoleDto) => (
        <Space size={4}>
          <Button data-testid="role-edit" size="small" onClick={() => selectRole(record)}>
            {t('roles.action.editPermissions')}
          </Button>
          <Button data-testid="role-delete" danger size="small" onClick={() => handleDelete(record)}>
            {t('btn.delete')}
          </Button>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <PageHeader
        title={t('roles.title')}
        subtitle={t('roles.subtitle')}
        actions={
          <>
            <Button data-testid="roles-refresh" icon={<ReloadOutlined />} onClick={() => refetch()}>
              {t('btn.refresh')}
            </Button>
            <Button data-testid="role-create" type="primary" onClick={() => setCreateOpen(true)}>
              {t('btn.create')}
            </Button>
          </>
        }
      />

      <div style={{ display: 'flex', gap: 16, alignItems: 'flex-start', flexWrap: 'wrap' }}>
        <div style={{ flex: '1 1 480px', minWidth: 0 }}>
          <Table
            dataSource={roles}
            columns={columns}
            rowKey="id"
            loading={isLoading}
            pagination={{ pageSize: 20, showTotal: (n) => t('table.totalItems', { n }) }}
            rowClassName={(r) => (r.id === selectedId ? 'ant-table-row-selected' : '')}
            onRow={(r) => ({ onClick: () => selectRole(r) })}
          />
        </div>

        <Card
          style={{ flex: '1 1 320px', minWidth: 280 }}
          title={
            selectedRole
              ? t('roles.matrix.titleFor', { name: selectedRole.name })
              : t('roles.matrix.title')
          }
          extra={
            <Button
              data-testid="role-permissions-save"
              type="primary"
              disabled={!selectedRole}
              loading={saveMutation.isPending}
              onClick={handleSave}
            >
              {t('roles.action.savePermissions')}
            </Button>
          }
        >
          {!selectedRole ? (
            <Empty description={t('roles.matrix.empty')} />
          ) : (
            catalog.map((g) => (
              <div key={g.group} style={{ marginBottom: 16 }}>
                <Title level={5} style={{ marginBottom: 8 }}>
                  {g.group}
                </Title>
                <Space direction="vertical" size={4}>
                  {g.keys.map((key) => (
                    <Checkbox
                      key={key}
                      checked={selectedKeys.has(key)}
                      onChange={(e) => toggleKey(key, e.target.checked)}
                    >
                      <Text code>{key}</Text>
                    </Checkbox>
                  ))}
                </Space>
              </div>
            ))
          )}
        </Card>
      </div>

      <Modal
        okButtonProps={{ 'data-testid': 'role-confirm' }}
        cancelButtonProps={{ 'data-testid': 'role-cancel' }}
        title={t('roles.modal.create')}
        open={createOpen}
        onOk={handleCreate}
        confirmLoading={createMutation.isPending}
        onCancel={() => setCreateOpen(false)}
        okText={t('btn.create')}
        cancelText={t('btn.cancel')}
        destroyOnClose
      >
        <Form form={createForm} layout="vertical" preserve={false}>
          <Form.Item name="name" label={t('roles.field.name')} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label={t('roles.field.description')}>
            <Input.TextArea rows={2} />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}
