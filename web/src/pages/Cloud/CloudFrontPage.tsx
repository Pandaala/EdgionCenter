import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, Button, Descriptions, Empty, Form, Input, Modal, Popconfirm, Select, Space, Table, Tag, Typography } from 'antd'
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import type { ProviderAccount } from '@/api/cloud'
import { cloudApi } from '@/api/cloud'
import { cloudfrontApi, cloudfrontMutationResult, type CloudFrontDistribution } from '@/api/cloudfront'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useCan } from '@/utils/permissions'

const CLOUDFRONT_KEY = ['cloudfront-distributions']
type MutationResult = 'accepted' | 'rejected' | 'conflicted' | 'ambiguous' | null

interface CreateValues { callerReference: string; originDomainName: string; originHttpsPort: number }
interface OriginValues { originDomainName: string; originHttpsPort: number }

function selectedAwsAccount(accounts: ProviderAccount[], accountId: string | undefined): ProviderAccount | undefined {
  return accounts.find((account) => account.provider === 'aws' && account.accountId === accountId)
}

/** A CloudFront origin update always reads the current Distribution immediately before dispatch. */
export async function freshOriginUpdate(accountId: string, distribution: CloudFrontDistribution, request: OriginValues) {
  await cloudfrontApi.observe(accountId, distribution.id)
  return cloudfrontApi.updateOrigin(accountId, distribution.id, { ...request, originHttpsPort: Number(request.originHttpsPort) })
}

export default function CloudFrontPage({ writeAvailable = false }: { writeAvailable?: boolean }) {
  const t = useT()
  const queryClient = useQueryClient()
  const canRead = useCan('cloudfront:read')
  const canAccounts = useCan('provider-accounts:read')
  const canWrite = useCan('cloudfront:write') && writeAvailable
  const canDisable = useCan('cloudfront:disable') && writeAvailable
  const canDelete = useCan('cloudfront:delete') && writeAvailable
  const canAccess = canRead && canAccounts
  const [accountId, setAccountId] = useState<string>()
  const [selectedId, setSelectedId] = useState<string>()
  const [editingOrigin, setEditingOrigin] = useState<CloudFrontDistribution | undefined>()
  const [result, setResult] = useState<MutationResult>(null)
  const [requiresRefresh, setRequiresRefresh] = useState(false)
  const [acceptedId, setAcceptedId] = useState<string>()
  const [createForm] = Form.useForm<CreateValues>()
  const [originForm] = Form.useForm<OriginValues>()
  const [deleteForm] = Form.useForm<{ confirmation: string }>()
  const accountsQuery = useQuery({ queryKey: ['cloud-provider-accounts'], queryFn: cloudApi.listAccounts, enabled: canAccounts })
  const accounts = useMemo(() => (accountsQuery.data?.data ?? []).filter((account) => account.provider === 'aws'), [accountsQuery.data?.data])
  const selectedAccount = selectedAwsAccount(accounts, accountId)
  const distributions = useQuery({ queryKey: [...CLOUDFRONT_KEY, accountId], queryFn: () => cloudfrontApi.list(accountId!), enabled: canAccess && accountId !== undefined })
  const selected = useQuery({ queryKey: [...CLOUDFRONT_KEY, accountId, selectedId, 'observation'], queryFn: () => cloudfrontApi.observe(accountId!, selectedId!), enabled: canAccess && accountId !== undefined && selectedId !== undefined })
  const items = distributions.data?.data ?? []
  const detail = selected.data?.data
  const fail = (error: unknown) => {
    const next = cloudfrontMutationResult(error)
    setResult(next)
    if (next === 'ambiguous') setRequiresRefresh(true)
  }
  const accept = (distribution: CloudFrontDistribution) => {
    setAcceptedId(distribution.id)
    setResult('accepted')
    queryClient.invalidateQueries({ queryKey: CLOUDFRONT_KEY })
  }
  const create = useMutation({
    mutationFn: (values: CreateValues) => cloudfrontApi.create(accountId!, { ...values, originHttpsPort: Number(values.originHttpsPort) }),
    onSuccess: (response) => {
      const item = response.data
      if (item) {
        setSelectedId(item.id)
        accept(item)
      }
      createForm.resetFields()
    },
    onError: fail,
  })
  const updateOrigin = useMutation({ mutationFn: ({ distribution, values }: { distribution: CloudFrontDistribution; values: OriginValues }) => freshOriginUpdate(accountId!, distribution, values), onSuccess: (response) => { const item = response.data; if (item) accept(item); setEditingOrigin(undefined) }, onError: fail })
  const enable = useMutation({ mutationFn: (distribution: CloudFrontDistribution) => cloudfrontApi.enable(accountId!, distribution.id), onSuccess: (response) => { if (response.data) accept(response.data) }, onError: fail })
  const disable = useMutation({ mutationFn: (distribution: CloudFrontDistribution) => cloudfrontApi.disable(accountId!, distribution.id), onSuccess: (response) => { if (response.data) accept(response.data) }, onError: fail })
  const remove = useMutation({ mutationFn: (distribution: CloudFrontDistribution) => cloudfrontApi.delete(accountId!, distribution.id), onSuccess: () => { setSelectedId(undefined); setResult('accepted'); queryClient.invalidateQueries({ queryKey: CLOUDFRONT_KEY }) }, onError: fail })
  const busy = create.isPending || updateOrigin.isPending || enable.isPending || disable.isPending || remove.isPending
  const mutationAllowed = !busy && !requiresRefresh
  const refresh = async () => {
    const list = await distributions.refetch()
    if (list.isError) return
    if (selectedId) {
      const observation = await selected.refetch()
      if (observation.isError) return
    }
    setRequiresRefresh(false)
    setResult(null)
    setAcceptedId(undefined)
  }
  const submitOrigin = async () => {
    const values = await originForm.validateFields()
    if (editingOrigin) updateOrigin.mutate({ distribution: editingOrigin, values })
  }
  const statusAlert = result === null ? null : <Alert showIcon style={{ marginBottom: 16 }} type={result === 'accepted' ? 'success' : result === 'ambiguous' ? 'warning' : 'error'} message={t(`cloud.cloudfront.result.${result}`)} />

  return <div>
    <PageHeader title={t('cloud.cloudfront.title')} subtitle={t('cloud.cloudfront.subtitle')} actions={<Button icon={<ReloadOutlined />} onClick={refresh} loading={distributions.isFetching || selected.isFetching}>{t('btn.refresh')}</Button>} />
    {!canRead && <Alert type="warning" showIcon message={t('cloud.cloudfront.permissionDenied')} />}
    {canRead && !canAccounts && <Alert type="warning" showIcon message={t('cloud.cloudfront.accountDenied')} />}
    {requiresRefresh && <Alert type="warning" showIcon style={{ marginTop: 16 }} message={t('cloud.cloudfront.refreshRequired')} />}
    {canAccess && <Space direction="vertical" size={16} style={{ width: '100%', marginTop: 16 }}>
      {statusAlert}
      <Space wrap><Typography.Text>{t('cloud.cloudfront.account')}</Typography.Text><Select data-testid="cloudfront-account" style={{ minWidth: 260 }} value={accountId} onChange={(value) => { setAccountId(value); setSelectedId(undefined); setResult(null); setRequiresRefresh(false); setAcceptedId(undefined) }} placeholder={t('cloud.cloudfront.selectAccount')} options={accounts.map((account) => ({ value: account.accountId, label: `${account.displayName} (${account.accountId})` }))} /></Space>
      {!selectedAccount && accountId === undefined && <Alert type="info" showIcon message={t('cloud.cloudfront.accountHint')} />}
      {selectedAccount && <Table size="small" rowKey="id" loading={distributions.isLoading} dataSource={items} title={() => <Space><Typography.Text strong>{t('cloud.cloudfront.distributions')}</Typography.Text>{canWrite && <Button data-testid="cloudfront-create" size="small" type="primary" icon={<PlusOutlined />} disabled={!mutationAllowed} onClick={() => Modal.confirm({ title: t('cloud.cloudfront.create'), content: <Form form={createForm} layout="vertical"><Form.Item name="callerReference" label={t('cloud.cloudfront.callerReference')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="originDomainName" label={t('cloud.cloudfront.originDomainName')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="originHttpsPort" label={t('cloud.cloudfront.originHttpsPort')} initialValue={443} rules={[{ required: true }]}><Input type="number" min={1} max={65535} /></Form.Item></Form>, onOk: async () => { const values = await createForm.validateFields(); create.mutate(values) }, okText: t('btn.create'), cancelText: t('btn.cancel') })}>{t('btn.create')}</Button>}</Space>} columns={[
        { title: t('cloud.cloudfront.id'), dataIndex: 'id' }, { title: t('cloud.cloudfront.domain'), dataIndex: 'domainName' }, { title: t('cloud.cloudfront.status'), dataIndex: 'status' },
        { title: t('cloud.cloudfront.enabled'), render: (_, item: CloudFrontDistribution) => <Tag color={item.enabled ? 'green' : 'default'}>{t(item.enabled ? 'cloud.cloudfront.enabled.true' : 'cloud.cloudfront.enabled.false')}</Tag> },
        { title: t('cloud.cloudfront.deployed'), render: (_, item: CloudFrontDistribution) => <Tag color={item.deployed ? 'green' : 'gold'}>{t(item.deployed ? 'cloud.cloudfront.deployed.true' : 'cloud.cloudfront.deployed.false')}</Tag> },
        { title: t('col.actions'), render: (_, item: CloudFrontDistribution) => <Space><Button size="small" onClick={() => setSelectedId(item.id)}>{t('btn.view')}</Button>{canWrite && <Button size="small" disabled={!mutationAllowed} onClick={() => { const current = detail?.id === item.id ? detail : item; originForm.setFieldsValue({ originDomainName: current.supportedOrigin?.domainName, originHttpsPort: current.supportedOrigin?.httpsPort ?? 443 }); setEditingOrigin(current) }}>{t('cloud.cloudfront.updateOrigin')}</Button>}{canWrite && !item.enabled && <Button size="small" disabled={!mutationAllowed} onClick={() => enable.mutate(item)}>{t('cloud.cloudfront.enable')}</Button>}{canDisable && item.enabled && <Popconfirm title={t('cloud.cloudfront.disableConfirm', { id: item.id })} onConfirm={() => disable.mutate(item)} okText={t('cloud.cloudfront.disable')} cancelText={t('btn.cancel')}><Button danger size="small" disabled={!mutationAllowed}>{t('cloud.cloudfront.disable')}</Button></Popconfirm>}{canDelete && !item.enabled && <Button danger size="small" disabled={!mutationAllowed} onClick={() => { deleteForm.resetFields(); Modal.confirm({ title: t('cloud.cloudfront.delete'), content: <Space direction="vertical"><Alert type="warning" showIcon message={t('cloud.cloudfront.deleteHint')} /><Form form={deleteForm} layout="vertical"><Form.Item name="confirmation" label={t('cloud.cloudfront.confirmDistribution', { id: item.id })} rules={[{ required: true }, { validator: (_, value) => value === item.id ? Promise.resolve() : Promise.reject(new Error(t('cloud.cloudfront.confirmMismatch'))) }]}><Input autoComplete="off" /></Form.Item></Form></Space>, onOk: async () => { await deleteForm.validateFields(); remove.mutate(item) }, okText: t('btn.delete'), cancelText: t('btn.cancel') }) }}>{t('btn.delete')}</Button>}</Space> },
      ]} />}
      {selectedAccount && !distributions.isLoading && items.length === 0 && <Empty description={t('cloud.cloudfront.noDistributions')} />}
      {detail && <Descriptions title={t('cloud.cloudfront.detail')} size="small" bordered column={2}><Descriptions.Item label={t('cloud.cloudfront.id')}>{detail.id}</Descriptions.Item><Descriptions.Item label={t('cloud.cloudfront.domain')}>{detail.domainName}</Descriptions.Item><Descriptions.Item label={t('cloud.cloudfront.status')}>{detail.status}</Descriptions.Item><Descriptions.Item label={t('cloud.cloudfront.deployed')}><Tag color={detail.deployed ? 'green' : 'gold'}>{t(detail.deployed ? 'cloud.cloudfront.deployed.true' : 'cloud.cloudfront.deployed.false')}</Tag></Descriptions.Item><Descriptions.Item label={t('cloud.cloudfront.providerAcceptance')}><Tag color={acceptedId === detail.id ? 'blue' : 'default'}>{t(acceptedId === detail.id ? 'cloud.cloudfront.accepted' : 'cloud.cloudfront.notObserved')}</Tag></Descriptions.Item><Descriptions.Item label={t('cloud.cloudfront.webAcl')}><Space>{detail.webAclId ?? '—'}{detail.webAclId && <Button size="small" type="link" href="/cloud/aws/waf">{t('cloud.cloudfront.wafPlaceholder')}</Button>}</Space></Descriptions.Item></Descriptions>}
    </Space>}
    <Modal title={t('cloud.cloudfront.updateOrigin')} open={editingOrigin !== undefined} onCancel={() => setEditingOrigin(undefined)} onOk={submitOrigin} confirmLoading={updateOrigin.isPending} okText={t('btn.save')} cancelText={t('btn.cancel')} destroyOnClose>
      <Alert type="info" showIcon message={t('cloud.cloudfront.freshOriginHint')} style={{ marginBottom: 16 }} />
      <Form form={originForm} layout="vertical" preserve={false}><Form.Item name="originDomainName" label={t('cloud.cloudfront.originDomainName')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="originHttpsPort" label={t('cloud.cloudfront.originHttpsPort')} rules={[{ required: true }]}><Input type="number" min={1} max={65535} /></Form.Item></Form>
    </Modal>
  </div>
}
