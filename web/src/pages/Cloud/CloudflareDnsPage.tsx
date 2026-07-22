import { useMemo, useState } from 'react'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, Button, Empty, Form, Input, Modal, Popconfirm, Select, Space, Table, Tag, Typography, message } from 'antd'
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import type { AxiosError } from 'axios'
import { cloudApi, type ProviderAccount } from '@/api/cloud'
import { cloudflareDnsApi, type CloudflareRecordPutRequest, type CloudflareRecordSet, type CloudflareRecordType, type CloudflareRecordValue, type CloudflareTtl, type CloudflareZone } from '@/api/cloudflareDns'
import { useCan } from '@/utils/permissions'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'

const DNS_KEY = ['cloudflare-dns']
const EDITABLE_TYPES: CloudflareRecordType[] = ['A', 'AAAA', 'CNAME', 'TXT']

function isSafelyEditable(record: CloudflareRecordSet): boolean {
  return EDITABLE_TYPES.includes(record.recordType) && record.values.length === 1
}

interface RecordFormValues {
  owner: string
  recordType: CloudflareRecordType
  ttlMode: 'automatic' | 'seconds'
  ttlSeconds?: number
  proxy?: 'dns_only' | 'proxied'
  cnameFlattening: 'provider_default' | 'flatten' | 'do_not_flatten'
  comment?: string
  tags?: string
  value: string
}

type MutationResult = 'applied' | 'rejected' | 'conflicted' | 'ambiguous' | null

function recordValueText(value: CloudflareRecordValue): string {
  if (value.type === 'A' || value.type === 'AAAA') return value.address
  if (value.type === 'CNAME' || value.type === 'NS') return value.target
  if (value.type === 'TXT') return value.segments.map((segment) => segment.base64).join(', ')
  if (value.type === 'MX') return `${value.preference} ${value.exchange}`
  if (value.type === 'SRV') return `${value.priority} ${value.weight} ${value.port} ${value.target}`
  if (value.type === 'CAA') return `${value.flags} ${value.tag} ${value.value.base64}`
  return `${value.primaryNameServer} ${value.responsibleMailbox}`
}

function firstEditableValue(record: CloudflareRecordSet): string {
  const first = record.values[0]
  return first ? recordValueText(first) : ''
}

function valueFromForm(type: CloudflareRecordType, value: string): CloudflareRecordValue {
  if (type === 'A') return { type, address: value }
  if (type === 'AAAA') return { type, address: value }
  if (type === 'CNAME') return { type, target: value }
  return { type: 'TXT', segments: value.split(',').map((segment) => ({ base64: segment.trim() })).filter((segment) => segment.base64.length > 0) }
}

function asRequest(values: RecordFormValues, existing?: CloudflareRecordSet): CloudflareRecordPutRequest {
  const ttl: CloudflareTtl = values.ttlMode === 'seconds' ? { type: 'seconds', seconds: Number(values.ttlSeconds) } : { type: 'automatic' }
  const canProxy = values.recordType === 'A' || values.recordType === 'AAAA' || values.recordType === 'CNAME'
  return {
    guard: existing ? { type: 'match_revision', revision: existing.revision } : { type: 'must_not_exist' },
    ttl,
    values: [valueFromForm(values.recordType, values.value)],
    ...(canProxy ? { proxy: values.proxy ?? 'dns_only' } : {}),
    cnameFlattening: values.cnameFlattening,
    comment: values.comment || undefined,
    tags: values.tags ? values.tags.split(',').map((tag) => tag.trim()).filter(Boolean) : [],
  }
}

function resultForError(error: unknown): MutationResult {
  const status = (error as AxiosError<{ error?: string }>).response?.status
  const code = (error as AxiosError<{ error?: string }>).response?.data?.error
  if (code === 'unknown_outcome') return 'ambiguous'
  if (status === 409 || status === 412) return 'conflicted'
  return 'rejected'
}

function selectedCloudflareAccount(accounts: ProviderAccount[], accountId: string | undefined): ProviderAccount | undefined {
  return accounts.find((account) => account.accountId === accountId && account.provider === 'cloudflare')
}

export default function CloudflareDnsPage() {
  const t = useT()
  const queryClient = useQueryClient()
  const canRead = useCan('cloudflare-dns:read')
  const canReadAccounts = useCan('provider-accounts:read')
  const canWrite = useCan('cloudflare-dns:write')
  const canDnsAccess = canRead && canReadAccounts
  const [accountId, setAccountId] = useState<string>()
  const [zone, setZone] = useState<CloudflareZone>()
  const [editing, setEditing] = useState<CloudflareRecordSet | null | undefined>(undefined)
  const [result, setResult] = useState<MutationResult>(null)
  const [zoneForm] = Form.useForm<{ name: string }>()
  const [recordForm] = Form.useForm<RecordFormValues>()
  const accountQuery = useQuery({ queryKey: ['cloud-provider-accounts'], queryFn: cloudApi.listAccounts, enabled: canReadAccounts })
  const accounts = useMemo(() => (accountQuery.data?.data ?? []).filter((account) => account.provider === 'cloudflare'), [accountQuery.data?.data])
  const selected = selectedCloudflareAccount(accounts, accountId)
  const zones = useInfiniteQuery({
    queryKey: [...DNS_KEY, accountId, 'zones'],
    queryFn: ({ pageParam }) => cloudflareDnsApi.listZones(accountId!, pageParam),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.data?.nextCursor,
    enabled: canDnsAccess && accountId !== undefined,
  })
  const records = useInfiniteQuery({
    queryKey: [...DNS_KEY, accountId, zone?.zoneId, 'records'],
    queryFn: ({ pageParam }) => cloudflareDnsApi.listRecords(accountId!, zone!.zoneId, pageParam),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.data?.nextCursor,
    enabled: canDnsAccess && accountId !== undefined && zone !== undefined,
  })
  const zoneItems = zones.data?.pages.flatMap((page) => page.data?.items ?? []) ?? []
  const recordItems = records.data?.pages.flatMap((page) => page.data?.items ?? []) ?? []
  const invalidateDns = () => {
    queryClient.invalidateQueries({ queryKey: DNS_KEY })
  }
  const refresh = () => {
    invalidateDns()
    setResult(null)
  }
  const createZone = useMutation({
    mutationFn: (name: string) => cloudflareDnsApi.createZone(accountId!, name),
    onSuccess: () => { message.success(t('cloud.dns.zoneCreated')); zoneForm.resetFields(); invalidateDns() },
    onError: (error) => setResult(resultForError(error)),
  })
  const deleteZone = useMutation({
    mutationFn: (target: CloudflareZone) => cloudflareDnsApi.deleteZone(accountId!, target),
    onSuccess: () => { setZone(undefined); setResult('applied'); message.success(t('cloud.dns.zoneDeleted')); invalidateDns() },
    onError: (error) => setResult(resultForError(error)),
  })
  const writeRecord = useMutation({
    mutationFn: ({ record, request }: { record: CloudflareRecordSet; request: CloudflareRecordPutRequest }) => cloudflareDnsApi.putRecord(accountId!, zone!.zoneId, record, request),
    onSuccess: () => { setResult('applied'); setEditing(undefined); message.success(t('cloud.dns.recordApplied')); invalidateDns() },
    onError: (error) => setResult(resultForError(error)),
  })
  const deleteRecord = useMutation({
    mutationFn: (record: CloudflareRecordSet) => cloudflareDnsApi.deleteRecord(accountId!, zone!.zoneId, record),
    onSuccess: () => { setResult('applied'); message.success(t('cloud.dns.recordDeleted')); invalidateDns() },
    onError: (error) => setResult(resultForError(error)),
  })
  const openEditor = (record: CloudflareRecordSet | null) => {
    setResult(null)
    setEditing(record)
    recordForm.setFieldsValue(record ? {
      owner: record.owner,
      recordType: record.recordType,
      ttlMode: record.ttl.type,
      ttlSeconds: record.ttl.type === 'seconds' ? record.ttl.seconds : undefined,
      proxy: record.proxy,
      cnameFlattening: record.cnameFlattening,
      comment: record.comment,
      tags: record.tags.join(', '),
      value: firstEditableValue(record),
    } : { recordType: 'A', ttlMode: 'automatic', proxy: 'dns_only', cnameFlattening: 'provider_default' })
  }
  const submitRecord = async () => {
    const values = await recordForm.validateFields()
    if (editing && !isSafelyEditable(editing)) return
    const shell: CloudflareRecordSet = editing ?? {
      providerAccountId: accountId!, zoneId: zone!.zoneId, zoneApex: zone!.name, zoneVisibility: zone!.visibility,
      owner: values.owner, recordType: values.recordType, ttl: { type: 'automatic' }, values: [], cnameFlattening: 'provider_default', tags: [], control: { type: 'manual' }, providerObjectIds: [], revision: '',
    }
    writeRecord.mutate({ record: shell, request: asRequest(values, editing ?? undefined) })
  }
  const recordType = Form.useWatch('recordType', recordForm)
  const ttlMode = Form.useWatch('ttlMode', recordForm)
  const proxyEnabled = recordType === 'A' || recordType === 'AAAA' || recordType === 'CNAME'
  const statusAlert = result === null ? null : <Alert style={{ marginBottom: 16 }} showIcon type={result === 'applied' ? 'success' : result === 'ambiguous' ? 'warning' : 'error'} message={t(`cloud.dns.result.${result}`)} />
  return (
    <div>
      <PageHeader title={t('cloud.dns.title')} subtitle={t('cloud.dns.subtitle')} actions={<Button icon={<ReloadOutlined />} onClick={refresh}>{t('btn.refresh')}</Button>} />
      {!canRead && <Alert type="warning" showIcon message={t('cloud.permission.dnsDenied')} />}
      {canRead && !canReadAccounts && <Alert type="warning" showIcon message={t('cloud.permission.dnsAccountDenied')} />}
      {canDnsAccess && <Space direction="vertical" size={16} style={{ width: '100%' }}>
        {statusAlert}
        <Space wrap>
          <Typography.Text>{t('cloud.dns.account')}</Typography.Text>
          <Select data-testid="cloudflare-dns-account" style={{ minWidth: 260 }} value={accountId} onChange={(value) => { setAccountId(value); setZone(undefined); setResult(null) }} options={accounts.map((account) => ({ value: account.accountId, label: `${account.displayName} (${account.accountId})` }))} placeholder={t('cloud.dns.selectAccount')} />
        </Space>
        {!selected && accountId === undefined && <Alert type="info" showIcon message={t('cloud.dns.accountHint')} />}
        {selected && <Table size="small" rowKey="zoneId" loading={zones.isLoading} dataSource={zoneItems} title={() => <Space><Typography.Text strong>{t('cloud.dns.zones')}</Typography.Text>{canWrite && <Button data-testid="cloudflare-zone-create" size="small" icon={<PlusOutlined />} onClick={() => Modal.confirm({ title: t('cloud.dns.createZone'), content: <Form form={zoneForm} layout="vertical"><Form.Item name="name" label={t('cloud.dns.zoneName')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { const values = await zoneForm.validateFields(); createZone.mutate(values.name) }, okText: t('btn.create'), cancelText: t('btn.cancel') })}>{t('btn.create')}</Button>}{zones.hasNextPage && <Button data-testid="cloudflare-zone-load-more" size="small" loading={zones.isFetchingNextPage} onClick={() => zones.fetchNextPage()}>{t('cloud.dns.loadMore')}</Button>}</Space>} columns={[
          { title: t('cloud.col.name'), dataIndex: 'name' },
          { title: t('cloud.col.status'), dataIndex: 'status', render: (value: string) => <Tag color={value === 'active' ? 'green' : 'gold'}>{value}</Tag> },
          { title: t('cloud.dns.nameservers'), render: (_, item: CloudflareZone) => item.nameservers.join(', ') || '—' },
          { title: t('col.actions'), render: (_, item: CloudflareZone) => <Space><Button data-testid="cloudflare-zone-open" size="small" onClick={() => { setZone(item); setResult(null) }}>{t('cloud.dns.records')}</Button>{canWrite && item.revision && <Popconfirm title={t('cloud.dns.deleteZoneConfirm', { name: item.name })} onConfirm={() => deleteZone.mutate(item)} okText={t('btn.delete')} cancelText={t('btn.cancel')}><Button danger size="small">{t('btn.delete')}</Button></Popconfirm>}</Space> },
        ]} />}
        {zone && <Table size="small" rowKey={(record) => `${record.owner}/${record.recordType}`} loading={records.isLoading} dataSource={recordItems} title={() => <Space><Typography.Text strong>{t('cloud.dns.recordsFor', { zone: zone.name })}</Typography.Text>{canWrite && <Button data-testid="cloudflare-record-create" size="small" type="primary" icon={<PlusOutlined />} onClick={() => openEditor(null)}>{t('cloud.dns.createRecord')}</Button>}{records.hasNextPage && <Button data-testid="cloudflare-record-load-more" size="small" loading={records.isFetchingNextPage} onClick={() => records.fetchNextPage()}>{t('cloud.dns.loadMore')}</Button>}</Space>} columns={[
          { title: t('cloud.dns.owner'), dataIndex: 'owner' }, { title: t('cloud.dns.recordType'), dataIndex: 'recordType' },
          { title: t('cloud.dns.values'), render: (_, record: CloudflareRecordSet) => record.values.map(recordValueText).join('; ') },
          { title: t('cloud.dns.proxy'), render: (_, record: CloudflareRecordSet) => <Tag color={record.proxy === 'proxied' ? 'orange' : 'default'}>{t(`cloud.dns.proxy.${record.proxy ?? 'not_applicable'}`)}</Tag> },
          { title: t('cloud.dns.flattening'), render: (_, record: CloudflareRecordSet) => t(`cloud.dns.flattening.${record.cnameFlattening}`) },
          { title: t('cloud.dns.control'), render: (_, record: CloudflareRecordSet) => record.control.type === 'remote' ? <Tag color="blue">{t('cloud.dns.remote', { caller: record.control.callerAlias })}</Tag> : <Tag color={record.control.type === 'manual' ? 'default' : 'red'}>{t(`cloud.dns.control.${record.control.type}`)}</Tag> },
          { title: t('col.actions'), render: (_, record: CloudflareRecordSet) => <Space>{canWrite && isSafelyEditable(record) && <Button size="small" onClick={() => openEditor(record)}>{t('btn.edit')}</Button>}{canWrite && record.recordType !== 'SOA' && <Popconfirm title={t('cloud.dns.deleteRecordConfirm', { name: `${record.owner} ${record.recordType}` })} onConfirm={() => deleteRecord.mutate(record)} okText={t('btn.delete')} cancelText={t('btn.cancel')}><Button danger size="small">{t('btn.delete')}</Button></Popconfirm>}</Space> },
        ]} />}
        {zone === undefined && selected && !zones.isLoading && zoneItems.length === 0 && <Empty description={t('cloud.dns.noZones')} />}
      </Space>}
      <Modal title={t(editing ? 'cloud.dns.editRecord' : 'cloud.dns.createRecord')} open={editing !== undefined} onCancel={() => setEditing(undefined)} onOk={submitRecord} confirmLoading={writeRecord.isPending} okButtonProps={{ disabled: editing !== null && editing !== undefined && !isSafelyEditable(editing) }} okText={t('btn.save')} cancelText={t('btn.cancel')} destroyOnClose>
        {editing && !isSafelyEditable(editing) && <Alert type="info" showIcon message={t('cloud.dns.readOnlyRecordType')} />}
        <Form form={recordForm} layout="vertical" preserve={false}>
          <Form.Item name="owner" label={t('cloud.dns.owner')} rules={[{ required: true }]}><Input disabled={editing !== null && editing !== undefined} autoComplete="off" /></Form.Item>
          <Form.Item name="recordType" label={t('cloud.dns.recordType')} rules={[{ required: true }]}><Select disabled={editing !== null && editing !== undefined} options={EDITABLE_TYPES.map((value) => ({ value, label: value }))} /></Form.Item>
          <Form.Item name="value" label={t(recordType === 'TXT' ? 'cloud.dns.base64Values' : 'cloud.dns.value')} rules={[{ required: true }]}><Input disabled={editing !== null && editing !== undefined && !isSafelyEditable(editing)} autoComplete="off" /></Form.Item>
          <Form.Item name="ttlMode" label={t('cloud.dns.ttl')}><Select options={['automatic', 'seconds'].map((value) => ({ value, label: t(`cloud.dns.ttl.${value}`) }))} /></Form.Item>
          {ttlMode === 'seconds' && <Form.Item name="ttlSeconds" label={t('cloud.dns.ttlSeconds')} rules={[{ required: true }]}><Input type="number" min={30} max={86400} /></Form.Item>}
          {proxyEnabled && <Form.Item name="proxy" label={t('cloud.dns.proxy')}><Select options={['dns_only', 'proxied'].map((value) => ({ value, label: t(`cloud.dns.proxy.${value}`) }))} /></Form.Item>}
          <Form.Item name="cnameFlattening" label={t('cloud.dns.flattening')}><Select options={['provider_default', 'flatten', 'do_not_flatten'].map((value) => ({ value, label: t(`cloud.dns.flattening.${value}`) }))} /></Form.Item>
          <Form.Item name="comment" label={t('cloud.dns.comment')}><Input autoComplete="off" /></Form.Item>
          <Form.Item name="tags" label={t('cloud.dns.tags')}><Input autoComplete="off" /></Form.Item>
        </Form>
      </Modal>
    </div>
  )
}
