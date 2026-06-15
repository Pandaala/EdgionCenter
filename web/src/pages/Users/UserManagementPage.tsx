import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Table, Button, Space, Tag, Modal, Form, Input, Select, message } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { usersApi, type UserDto } from '@/api/users'
import { rolesApi } from '@/api/roles'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'

const USERS_KEY = ['center-admin-users']

export default function UserManagementPage() {
  const t = useT()
  const queryClient = useQueryClient()

  const [createForm] = Form.useForm()
  const [pwForm] = Form.useForm()
  const [rolesForm] = Form.useForm()
  const [createOpen, setCreateOpen] = useState(false)
  // The user currently targeted by the password / role-edit modals.
  const [pwTarget, setPwTarget] = useState<UserDto | null>(null)
  const [rolesTarget, setRolesTarget] = useState<UserDto | null>(null)

  const { data, isLoading, refetch } = useQuery({
    queryKey: USERS_KEY,
    queryFn: usersApi.list,
    staleTime: 30000,
  })
  const users: UserDto[] = data?.data ?? []

  const { data: rolesData } = useQuery({
    queryKey: ['center-admin-roles'],
    queryFn: rolesApi.list,
    staleTime: 30000,
  })
  const roleOptions = (rolesData?.data ?? []).map((r) => ({ label: r.name, value: r.id }))

  const invalidate = () => queryClient.invalidateQueries({ queryKey: USERS_KEY })

  const createMutation = useMutation({
    mutationFn: usersApi.create,
    onSuccess: () => {
      message.success(t('users.msg.createOk'))
      setCreateOpen(false)
      createForm.resetFields()
      invalidate()
    },
    onError: (e: any) => message.error(t('users.msg.createFail', { err: e.message })),
  })

  const statusMutation = useMutation({
    mutationFn: ({ id, status }: { id: number; status: string }) => usersApi.update(id, { status }),
    onSuccess: () => {
      message.success(t('users.msg.statusOk'))
      invalidate()
    },
    onError: (e: any) => message.error(t('users.msg.updateFail', { err: e.message })),
  })

  const passwordMutation = useMutation({
    mutationFn: ({ id, password }: { id: number; password: string }) =>
      usersApi.update(id, { password }),
    onSuccess: () => {
      message.success(t('users.msg.passwordOk'))
      setPwTarget(null)
      pwForm.resetFields()
    },
    onError: (e: any) => message.error(t('users.msg.updateFail', { err: e.message })),
  })

  const rolesMutation = useMutation({
    mutationFn: ({ id, roleIds }: { id: number; roleIds: number[] }) =>
      usersApi.update(id, { roleIds }),
    onSuccess: () => {
      message.success(t('users.msg.rolesOk'))
      setRolesTarget(null)
      rolesForm.resetFields()
      invalidate()
    },
    onError: (e: any) => message.error(t('users.msg.updateFail', { err: e.message })),
  })

  const deleteMutation = useMutation({
    mutationFn: (id: number) => usersApi.remove(id),
    onSuccess: () => {
      message.success(t('users.msg.deleteOk'))
      invalidate()
    },
    onError: (e: any) => message.error(t('users.msg.deleteFail', { err: e.message })),
  })

  const handleCreate = async () => {
    const values = await createForm.validateFields()
    createMutation.mutate({
      username: values.username,
      password: values.password,
      displayName: values.displayName || undefined,
      roleIds: values.roleIds ?? [],
    })
  }

  const handleResetPassword = async () => {
    if (!pwTarget) return
    const values = await pwForm.validateFields()
    passwordMutation.mutate({ id: pwTarget.id, password: values.password })
  }

  const handleSaveRoles = async () => {
    if (!rolesTarget) return
    const values = await rolesForm.validateFields()
    rolesMutation.mutate({ id: rolesTarget.id, roleIds: values.roleIds ?? [] })
  }

  const handleDelete = (record: UserDto) => {
    Modal.confirm({
      title: t('confirm.deleteTitle'),
      content: t('users.confirm.delete', { name: record.username }),
      okText: t('confirm.okText'),
      okType: 'danger',
      cancelText: t('btn.cancel'),
      onOk: () => deleteMutation.mutate(record.id),
    })
  }

  const openRolesModal = (record: UserDto) => {
    setRolesTarget(record)
    rolesForm.setFieldsValue({ roleIds: record.roleIds })
  }

  const columns = [
    { title: t('users.col.username'), dataIndex: 'username', key: 'username' },
    { title: t('users.col.displayName'), dataIndex: 'displayName', key: 'displayName' },
    {
      title: t('users.col.status'),
      dataIndex: 'status',
      key: 'status',
      render: (v: string) =>
        v === 'active' ? (
          <Tag color="green">{t('users.status.active')}</Tag>
        ) : (
          <Tag color="red">{t('users.status.disabled')}</Tag>
        ),
    },
    {
      title: t('users.col.roles'),
      dataIndex: 'roleNames',
      key: 'roleNames',
      render: (v: string[]) =>
        v.length ? (
          <Space wrap size={4}>
            {v.map((n) => (
              <Tag key={n} color="blue">
                {n}
              </Tag>
            ))}
          </Space>
        ) : (
          '-'
        ),
    },
    {
      title: t('users.col.createdAt'),
      dataIndex: 'createdAt',
      key: 'createdAt',
      render: (v: number) => (v ? new Date(v * 1000).toLocaleString() : '-'),
    },
    {
      title: t('col.actions'),
      key: 'actions',
      render: (_: unknown, record: UserDto) => (
        <Space wrap size={4}>
          {record.status === 'active' ? (
            <Button size="small" onClick={() => statusMutation.mutate({ id: record.id, status: 'disabled' })}>
              {t('users.action.disable')}
            </Button>
          ) : (
            <Button size="small" onClick={() => statusMutation.mutate({ id: record.id, status: 'active' })}>
              {t('users.action.enable')}
            </Button>
          )}
          <Button size="small" onClick={() => setPwTarget(record)}>
            {t('users.action.resetPassword')}
          </Button>
          <Button size="small" onClick={() => openRolesModal(record)}>
            {t('users.action.editRoles')}
          </Button>
          <Button danger size="small" onClick={() => handleDelete(record)}>
            {t('btn.delete')}
          </Button>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <PageHeader
        title={t('users.title')}
        subtitle={t('users.subtitle')}
        actions={
          <>
            <Button icon={<ReloadOutlined />} onClick={() => refetch()}>
              {t('btn.refresh')}
            </Button>
            <Button type="primary" onClick={() => setCreateOpen(true)}>
              {t('btn.create')}
            </Button>
          </>
        }
      />

      <Table
        dataSource={users}
        columns={columns}
        rowKey="id"
        loading={isLoading}
        pagination={{ pageSize: 20, showTotal: (n) => t('table.totalItems', { n }) }}
      />

      <Modal
        title={t('users.modal.create')}
        open={createOpen}
        onOk={handleCreate}
        confirmLoading={createMutation.isPending}
        onCancel={() => setCreateOpen(false)}
        okText={t('btn.create')}
        cancelText={t('btn.cancel')}
        destroyOnClose
      >
        <Form form={createForm} layout="vertical" preserve={false}>
          <Form.Item name="username" label={t('users.field.username')} rules={[{ required: true }]}>
            <Input autoComplete="off" />
          </Form.Item>
          <Form.Item name="password" label={t('users.field.password')} rules={[{ required: true }]}>
            <Input.Password autoComplete="new-password" />
          </Form.Item>
          <Form.Item name="displayName" label={t('users.field.displayName')}>
            <Input />
          </Form.Item>
          <Form.Item name="roleIds" label={t('users.field.roles')}>
            <Select mode="multiple" allowClear options={roleOptions} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={t('users.modal.resetPassword')}
        open={pwTarget !== null}
        onOk={handleResetPassword}
        confirmLoading={passwordMutation.isPending}
        onCancel={() => setPwTarget(null)}
        okText={t('btn.save')}
        cancelText={t('btn.cancel')}
        destroyOnClose
      >
        <Form form={pwForm} layout="vertical" preserve={false}>
          <Form.Item name="password" label={t('users.field.password')} rules={[{ required: true }]}>
            <Input.Password autoComplete="new-password" />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={t('users.modal.editRoles')}
        open={rolesTarget !== null}
        onOk={handleSaveRoles}
        confirmLoading={rolesMutation.isPending}
        onCancel={() => setRolesTarget(null)}
        okText={t('btn.save')}
        cancelText={t('btn.cancel')}
        destroyOnClose
      >
        <Form form={rolesForm} layout="vertical" preserve={false}>
          <Form.Item name="roleIds" label={t('users.field.roles')}>
            <Select mode="multiple" allowClear options={roleOptions} />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}
