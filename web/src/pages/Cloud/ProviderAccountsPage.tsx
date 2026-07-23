import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, Button, Drawer, Form, Input, Modal, Select, Space, Table, Tag, Typography, message } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { cloudApi, type CredentialSource, type ProviderAccount, type ProviderAccountDesired, type ProviderCapabilityRead, type ProviderCapabilitySnapshot } from '@/api/cloud'
import { useCan } from '@/utils/permissions'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'

const ACCOUNTS_KEY = ['cloud-provider-accounts']

interface AccountFormValues {
  accountId: string
  displayName: string
  owner?: string
  managementPolicy: 'managed' | 'observe_only'
  provider: 'cloudflare' | 'aws'
  scopeId: string
  credentialType: CredentialSource['type']
  credentialRef?: string
  subjectTokenRef?: string
  baseCredentialRef?: string
  targetPrincipal?: string
  audience?: string
  externalIdRef?: string
}

function desiredFromValues(values: AccountFormValues): ProviderAccountDesired {
  const scope = { provider: values.provider, accountId: values.scopeId } as ProviderAccountDesired['scope']
  const credentialSource: CredentialSource = values.credentialType === 'ambient'
    ? { type: 'ambient' }
    : values.credentialType === 'static_secret'
      ? { type: 'static_secret', credentialRef: values.credentialRef ?? '' }
      : values.credentialType === 'federated'
        ? { type: 'federated', subjectTokenRef: values.subjectTokenRef || undefined, targetPrincipal: values.targetPrincipal ?? '', audience: values.audience || undefined }
        : { type: 'assume_identity', baseCredentialRef: values.baseCredentialRef || undefined, targetPrincipal: values.targetPrincipal ?? '', externalIdRef: values.externalIdRef || undefined }
  return {
    displayName: values.displayName,
    owner: values.owner || undefined,
    labels: {},
    managementPolicy: values.managementPolicy,
    provider: values.provider,
    scope,
    credentialSource,
  }
}

function formValues(account: ProviderAccount): AccountFormValues {
  const source = account.credentialSource
  return {
    accountId: account.accountId,
    displayName: account.displayName,
    owner: account.owner,
    managementPolicy: account.managementPolicy,
    provider: account.provider,
    scopeId: account.scope.accountId,
    credentialType: source.type,
    ...(source.type === 'static_secret' ? { credentialRef: source.credentialRef } : {}),
    ...(source.type === 'federated' ? { subjectTokenRef: source.subjectTokenRef, targetPrincipal: source.targetPrincipal, audience: source.audience } : {}),
    ...(source.type === 'assume_identity' ? { baseCredentialRef: source.baseCredentialRef, targetPrincipal: source.targetPrincipal, externalIdRef: source.externalIdRef } : {}),
  }
}

function capabilityColor(state: string): string {
  if (state === 'affirmative') return 'green'
  if (state === 'negative') return 'red'
  if (state === 'unknown') return 'gold'
  return 'default'
}

function CapabilityDrawer({ account, onClose }: { account: ProviderAccount | null; onClose: () => void }) {
  const t = useT()
  const canRead = useCan('provider-capabilities:read')
  const canInspect = useCan('provider-credentials:inspect')
  const [inspection, setInspection] = useState<string | null>(null)
  const capability = useQuery({
    queryKey: ['cloud-capabilities', account?.accountId],
    queryFn: () => cloudApi.getCapabilities(account!.accountId),
    enabled: account !== null && canRead,
  })
  const inspectionMutation = useMutation({
    mutationFn: () => cloudApi.inspectCredentials(account!.accountId),
    onSuccess: (result) => setInspection(result.data?.state ?? null),
    onError: () => setInspection('failed'),
  })
  useEffect(() => { setInspection(null) }, [account?.accountId])
  const data: ProviderCapabilityRead | undefined = capability.data?.data
  const snapshot = data?.snapshot
  return (
    <Drawer title={t('cloud.capabilities.title')} open={account !== null} onClose={onClose} width={680} destroyOnClose>
      {!canRead && <Alert type="warning" showIcon message={t('cloud.permission.capabilityDenied')} />}
      {canRead && (
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          <Space wrap>
            <Button data-testid="cloud-capabilities-refresh" icon={<ReloadOutlined />} onClick={() => capability.refetch()} loading={capability.isFetching}>{t('cloud.action.refreshEvidence')}</Button>
            {canInspect && <Button data-testid="cloud-credential-inspect" onClick={() => inspectionMutation.mutate()} loading={inspectionMutation.isPending}>{t('cloud.action.inspectCredential')}</Button>}
          </Space>
          {inspection !== null && <Alert type={inspection === 'valid' ? 'success' : 'warning'} showIcon message={t(`cloud.credential.${inspection}`)} />}
          {!data || data.snapshotState === 'not_discovered' ? <Alert type="info" showIcon message={t('cloud.capabilities.notDiscovered')} /> : null}
          {snapshot && !snapshot.accountGenerationMatches && <Alert type="warning" showIcon message={t('cloud.capabilities.stale')} />}
          {snapshot?.issues.map((issue, index) => <Alert key={index} type={issue.severity === 'blocking' ? 'error' : 'warning'} showIcon message={t(`cloud.reason.${issue.reason}`)} />)}
          <Table
            size="small"
            rowKey={(item) => `${item.capability.family}/${item.capability.name}`}
            pagination={false}
            dataSource={snapshot?.observations ?? []}
            columns={[
              { title: t('cloud.col.capability'), render: (_, row) => `${row.capability.family}/${row.capability.name}` },
              { title: t('cloud.col.evidence'), render: (_: unknown, row: ProviderCapabilitySnapshot['observations'][number]) => <Space wrap>{row.dimensions.map((dimension) => <Tag key={`${dimension.dimension}/${dimension.action ?? ''}`} color={capabilityColor(dimension.state)}>{`${dimension.dimension}: ${t(`cloud.state.${dimension.state}`)}`}</Tag>)}</Space> },
            ]}
          />
        </Space>
      )}
    </Drawer>
  )
}

export default function ProviderAccountsPage() {
  const t = useT()
  const queryClient = useQueryClient()
  const canAccountWrite = useCan('provider-accounts:write')
  const canUseCredential = useCan('provider-credentials:use')
  const canWrite = canAccountWrite && canUseCredential
  const [form] = Form.useForm<AccountFormValues>()
  const [target, setTarget] = useState<ProviderAccount | null>(null)
  const [open, setOpen] = useState(false)
  const [capabilityAccount, setCapabilityAccount] = useState<ProviderAccount | null>(null)
  const accounts = useQuery({ queryKey: ACCOUNTS_KEY, queryFn: cloudApi.listAccounts })
  const close = () => { setOpen(false); setTarget(null); form.resetFields() }
  const create = useMutation({
    mutationFn: (values: AccountFormValues) => cloudApi.createAccount(values.accountId, desiredFromValues(values)),
    onSuccess: () => { message.success(t('cloud.msg.accountSaved')); queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY }); close() },
  })
  const replace = useMutation({
    mutationFn: async (values: AccountFormValues) => {
      const current = await cloudApi.getAccount(values.accountId)
      if (!current.etag) throw new Error('missing account revision')
      return cloudApi.replaceAccount(values.accountId, desiredFromValues(values), current.etag)
    },
    onSuccess: () => { message.success(t('cloud.msg.accountSaved')); queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY }); close() },
  })
  const openCreate = () => { form.setFieldsValue({ provider: 'cloudflare', managementPolicy: 'observe_only', credentialType: 'static_secret' }); setTarget(null); setOpen(true) }
  const openEdit = (account: ProviderAccount) => { form.setFieldsValue(formValues(account)); setTarget(account); setOpen(true) }
  const submit = async () => {
    const values = await form.validateFields()
    if (target) replace.mutate(values)
    else create.mutate(values)
  }
  const credentialType = Form.useWatch('credentialType', form)
  return (
    <div>
      <PageHeader title={t('cloud.accounts.title')} subtitle={t('cloud.accounts.subtitle')} actions={<Space><Button icon={<ReloadOutlined />} onClick={() => accounts.refetch()}>{t('btn.refresh')}</Button>{canWrite && <Button data-testid="cloud-account-create" type="primary" onClick={openCreate}>{t('cloud.action.createAccount')}</Button>}</Space>} />
      {!canWrite && <Alert type="info" showIcon message={t('cloud.permission.accountReadonly')} style={{ marginBottom: 16 }} />}
      <Table rowKey="accountId" loading={accounts.isLoading} dataSource={accounts.data?.data ?? []} pagination={{ pageSize: 20 }} columns={[
        { title: t('cloud.col.account'), dataIndex: 'accountId' },
        { title: t('cloud.col.provider'), dataIndex: 'provider', render: (value: string) => <Tag>{value}</Tag> },
        { title: t('cloud.col.scope'), render: (_, row: ProviderAccount) => row.scope.accountId },
        { title: t('cloud.col.generation'), dataIndex: 'generation' },
        { title: t('col.actions'), render: (_, row: ProviderAccount) => <Space><Button size="small" onClick={() => setCapabilityAccount(row)}>{t('cloud.action.capabilities')}</Button>{canWrite && <Button size="small" onClick={() => openEdit(row)}>{t('btn.edit')}</Button>}</Space> },
      ]} />
      <Modal title={t(target ? 'cloud.accounts.editTitle' : 'cloud.accounts.createTitle')} open={open} onCancel={close} onOk={submit} confirmLoading={create.isPending || replace.isPending} destroyOnClose okText={target ? t('btn.save') : t('btn.create')} cancelText={t('btn.cancel')}>
        <Form form={form} layout="vertical" preserve={false}>
          <Form.Item name="accountId" label={t('cloud.field.accountId')} rules={[{ required: true }]}><Input disabled={target !== null} autoComplete="off" /></Form.Item>
          <Form.Item name="displayName" label={t('cloud.field.displayName')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item>
          <Form.Item name="owner" label={t('cloud.field.owner')}><Input autoComplete="off" /></Form.Item>
          <Form.Item name="provider" label={t('cloud.field.provider')} rules={[{ required: true }]}><Select disabled={target !== null} options={['cloudflare', 'aws'].map((value) => ({ value, label: value }))} /></Form.Item>
          <Form.Item name="scopeId" label={t('cloud.field.nativeAccountId')} rules={[{ required: true }]}><Input disabled={target !== null} autoComplete="off" /></Form.Item>
          <Form.Item name="managementPolicy" label={t('cloud.field.managementPolicy')}><Select options={['observe_only', 'managed'].map((value) => ({ value, label: t(`cloud.policy.${value}`) }))} /></Form.Item>
          <Form.Item name="credentialType" label={t('cloud.field.credentialType')} rules={[{ required: true }]}><Select options={['static_secret', 'ambient', 'federated', 'assume_identity'].map((value) => ({ value, label: t(`cloud.credentialType.${value}`) }))} /></Form.Item>
          {credentialType === 'static_secret' && <Form.Item name="credentialRef" label={t('cloud.field.credentialRef')} rules={[{ required: true }]}><Input.Password autoComplete="new-password" /></Form.Item>}
          {credentialType === 'federated' && <><Form.Item name="targetPrincipal" label={t('cloud.field.targetPrincipal')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="subjectTokenRef" label={t('cloud.field.subjectTokenRef')}><Input.Password autoComplete="new-password" /></Form.Item><Form.Item name="audience" label={t('cloud.field.audience')}><Input autoComplete="off" /></Form.Item></>}
          {credentialType === 'assume_identity' && <><Form.Item name="targetPrincipal" label={t('cloud.field.targetPrincipal')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="baseCredentialRef" label={t('cloud.field.baseCredentialRef')}><Input.Password autoComplete="new-password" /></Form.Item><Form.Item name="externalIdRef" label={t('cloud.field.externalIdRef')}><Input.Password autoComplete="new-password" /></Form.Item></>}
          <Typography.Paragraph type="secondary">{t('cloud.accounts.secretReferenceNotice')}</Typography.Paragraph>
        </Form>
      </Modal>
      <CapabilityDrawer account={capabilityAccount} onClose={() => setCapabilityAccount(null)} />
    </div>
  )
}
