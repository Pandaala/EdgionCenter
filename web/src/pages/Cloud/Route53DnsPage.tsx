import { useMemo, useState } from 'react'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, Button, Descriptions, Empty, Form, Input, Modal, Popconfirm, Select, Space, Switch, Table, Tag, Typography } from 'antd'
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import { cloudApi, type ProviderAccount } from '@/api/cloud'
import {
  route53DnsApi,
  route53MutationResult,
  type Route53AliasTarget,
  type Route53RecordDesired,
  type Route53RecordSet,
  type Route53RecordType,
  type Route53RecordValue,
  type Route53RecordWriteValue,
  type Route53RoutingPolicy,
  type Route53Zone,
  type Route53ZoneLifecycleObservation,
} from '@/api/route53Dns'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useCan } from '@/utils/permissions'

const DNS_KEY = ['route53-dns']
const EDITABLE_TYPES: Route53RecordType[] = ['A', 'AAAA', 'CNAME', 'TXT']
type MutationResult = 'applied' | 'pending' | 'rejected' | 'conflicted' | 'ambiguous' | null

interface RecordFormValues {
  owner: string
  recordType: Route53RecordType
  setIdentifier?: string
  value?: string
  ttlSeconds?: number
  aliasEnabled?: boolean
  aliasTargetZoneId?: string
  aliasTarget?: string
  evaluateTargetHealth?: boolean
  routingKind?: 'none' | 'weighted' | 'failover' | 'latency' | 'geolocation' | 'multivalue'
  routingWeight?: number
  failoverRole?: 'primary' | 'secondary'
  latencyRegion?: string
  geoKind?: 'default' | 'continent' | 'country' | 'us_subdivision'
  geoCode?: string
  healthCheckId?: string
}

function recordValueText(value: Route53RecordValue): string {
  if (value.type === 'A' || value.type === 'AAAA') return value.address
  if (value.type === 'CNAME' || value.type === 'NS') return value.target
  if (value.type === 'TXT') return value.value.map(base64UrlFromBytes).join(', ')
  if (value.type === 'MX') return `${value.preference} ${value.exchange}`
  if (value.type === 'SRV') return `${value.priority} ${value.weight} ${value.port} ${value.target}`
  if (value.type === 'CAA') return `${value.flags} ${value.tag} ${base64UrlFromBytes(value.value)}`
  return `${value.primary_name_server} ${value.responsible_mailbox}`
}

function base64UrlFromBytes(bytes: number[]): string {
  const binary = String.fromCharCode(...bytes)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '')
}

function editableValue(record: Route53RecordSet): string {
  const value = record.recordSet.values[0]
  return value ? recordValueText(value) : ''
}

function valueFromForm(type: Route53RecordType, value: string): Route53RecordWriteValue {
  if (type === 'A') return { type, address: value }
  if (type === 'AAAA') return { type, address: value }
  if (type === 'CNAME') return { type, target: value }
  return { type: 'TXT', segments: value.split(',').map((base64) => ({ base64: base64.trim() })).filter((segment) => segment.base64.length > 0) }
}

function routingFromForm(values: RecordFormValues): Route53RoutingPolicy | undefined {
  switch (values.routingKind) {
    case 'weighted': return { type: 'weighted', weight: Number(values.routingWeight) }
    case 'failover': return { type: 'failover', role: values.failoverRole ?? 'primary' }
    case 'latency': return { type: 'latency', region: values.latencyRegion ?? '' }
    case 'geolocation': return { type: 'geolocation', location: values.geoKind === 'default' ? { type: 'default' } : { type: values.geoKind ?? 'country', code: values.geoCode ?? '' } }
    case 'multivalue': return { type: 'multivalue' }
    default: return undefined
  }
}

function aliasFromForm(values: RecordFormValues): Route53AliasTarget | undefined {
  if (!values.aliasEnabled) return undefined
  return {
    targetZoneId: values.aliasTargetZoneId ?? '',
    target: values.aliasTarget ?? '',
    evaluateTargetHealth: values.evaluateTargetHealth === true,
  }
}

/** Converts the typed editor state into the strict Route 53 wire contract. */
export function recordDesiredFromForm(values: RecordFormValues): Route53RecordDesired {
  const aliasTarget = aliasFromForm(values)
  return {
    ttl: aliasTarget ? { type: 'inherited' } : { type: 'seconds', seconds: Number(values.ttlSeconds) },
    values: aliasTarget ? [] : [valueFromForm(values.recordType, values.value ?? '')],
    ...(aliasTarget ? { aliasTarget } : {}),
    ...(routingFromForm(values) ? { routingPolicy: routingFromForm(values) } : {}),
    ...(values.healthCheckId ? { healthCheckId: values.healthCheckId } : {}),
  }
}

function isSafelyEditable(record: Route53RecordSet): boolean {
  return EDITABLE_TYPES.includes(record.recordSet.key.recordType) && record.recordSet.values.length <= 1
}

function selectedAwsAccount(accounts: ProviderAccount[], accountId: string | undefined): ProviderAccount | undefined {
  return accounts.find((account) => account.accountId === accountId && account.provider === 'aws')
}

function formFromRecord(record?: Route53RecordSet | null): RecordFormValues {
  const extension = record?.recordSet.extension
  const routing = extension?.routing_policy
  const alias = extension?.alias_target
  return {
    owner: record?.recordSet.key.owner ?? '',
    recordType: record?.recordSet.key.recordType ?? 'A',
    setIdentifier: record?.recordSet.key.routing.type === 'route53' ? record.recordSet.key.routing.set_identifier : undefined,
    value: record ? editableValue(record) : '',
    ttlSeconds: record?.recordSet.ttl.type === 'seconds' ? record.recordSet.ttl.seconds : 60,
    aliasEnabled: alias !== undefined,
    aliasTargetZoneId: alias?.targetZoneId,
    aliasTarget: alias?.target,
    evaluateTargetHealth: alias?.evaluateTargetHealth,
    routingKind: routing?.type ?? 'none',
    routingWeight: routing?.type === 'weighted' ? routing.weight : undefined,
    failoverRole: routing?.type === 'failover' ? routing.role : undefined,
    latencyRegion: routing?.type === 'latency' ? routing.region : undefined,
    geoKind: routing?.type === 'geolocation' ? routing.location.type : undefined,
    geoCode: routing?.type === 'geolocation' && routing.location.type !== 'default' ? routing.location.code : undefined,
    healthCheckId: extension?.health_check_id,
  }
}

export default function Route53DnsPage({ dnsWriteAvailable = false, zoneLifecycleAvailable = false }: { dnsWriteAvailable?: boolean; zoneLifecycleAvailable?: boolean }) {
  const t = useT()
  const queryClient = useQueryClient()
  const canRead = useCan('route53-dns:read')
  const canWrite = useCan('route53-dns:write') && dnsWriteAvailable
  const canZonesWrite = useCan('route53-zones:write') && zoneLifecycleAvailable
  const canAccounts = useCan('provider-accounts:read')
  const canDnsAccess = canRead && canAccounts
  const [accountId, setAccountId] = useState<string>()
  const [zone, setZone] = useState<Route53Zone>()
  const [editing, setEditing] = useState<Route53RecordSet | null | undefined>(undefined)
  const [lifecycle, setLifecycle] = useState<Route53ZoneLifecycleObservation>()
  const [result, setResult] = useState<MutationResult>(null)
  const [zoneForm] = Form.useForm<{ apex: string }>()
  const [recordForm] = Form.useForm<RecordFormValues>()
  const [deleteForm] = Form.useForm<{ confirmApex: string }>()
  const accountsQuery = useQuery({ queryKey: ['cloud-provider-accounts'], queryFn: cloudApi.listAccounts, enabled: canAccounts })
  const accounts = useMemo(() => (accountsQuery.data?.data ?? []).filter((account) => account.provider === 'aws'), [accountsQuery.data?.data])
  const selected = selectedAwsAccount(accounts, accountId)
  const zones = useInfiniteQuery({
    queryKey: [...DNS_KEY, accountId, 'zones'], queryFn: ({ pageParam }) => route53DnsApi.listZones(accountId!, pageParam),
    initialPageParam: undefined as string | undefined, getNextPageParam: (last) => last.data?.nextCursor, enabled: canDnsAccess && accountId !== undefined,
  })
  const records = useInfiniteQuery({
    queryKey: [...DNS_KEY, accountId, zone?.zoneId, 'records'], queryFn: ({ pageParam }) => route53DnsApi.listRecords(accountId!, zone!.zoneId, pageParam),
    initialPageParam: undefined as string | undefined, getNextPageParam: (last) => last.data?.nextCursor, enabled: canDnsAccess && accountId !== undefined && zone !== undefined,
  })
  const zoneItems = zones.data?.pages.flatMap((page) => page.data?.items ?? []) ?? []
  const recordItems = records.data?.pages.flatMap((page) => page.data?.items ?? []) ?? []
  const invalidate = () => { queryClient.invalidateQueries({ queryKey: DNS_KEY }) }
  const setError = (error: unknown) => setResult(route53MutationResult(error))
  const createZone = useMutation({
    mutationFn: (apex: string) => route53DnsApi.createZone(accountId!, apex, crypto.randomUUID()),
    onSuccess: () => { setResult('pending'); zoneForm.resetFields(); invalidate() }, onError: setError,
  })
  const inspectZone = useMutation({
    mutationFn: (target: Route53Zone) => route53DnsApi.observeZoneLifecycle(accountId!, target),
    onSuccess: (response) => setLifecycle(response.data), onError: setError,
  })
  const deleteZone = useMutation({
    mutationFn: async (target: Route53Zone) => {
      const observation = (await route53DnsApi.observeZoneLifecycle(accountId!, target)).data
      if (!observation || observation.nonDefaultRecordCount !== 0 || observation.dnssec.state !== 'disabled') throw new Error('unsafe_zone_delete')
      return route53DnsApi.deleteZone(accountId!, observation)
    },
    onSuccess: () => { setZone(undefined); setLifecycle(undefined); setResult('pending'); invalidate() }, onError: setError,
  })
  const putRecord = useMutation({
    mutationFn: ({ existing, desired }: { existing?: Route53RecordSet; desired: Route53RecordDesired }) => {
      const key = existing?.recordSet.key ?? {
        owner: recordForm.getFieldValue('owner'), recordType: recordForm.getFieldValue('recordType'),
        routing: recordForm.getFieldValue('setIdentifier') ? { type: 'route53' as const, set_identifier: recordForm.getFieldValue('setIdentifier') } : { type: 'simple' as const },
      }
      return route53DnsApi.putRecord(accountId!, zone!.zoneId, key, desired, existing?.revision)
    },
    onSuccess: () => { setEditing(undefined); setResult('pending'); invalidate() }, onError: setError,
  })
  const deleteRecord = useMutation({
    mutationFn: (record: Route53RecordSet) => route53DnsApi.deleteRecord(accountId!, zone!.zoneId, record),
    onSuccess: () => { setResult('pending'); invalidate() }, onError: setError,
  })
  const openEditor = (record: Route53RecordSet | null) => { setResult(null); setEditing(record); recordForm.setFieldsValue(formFromRecord(record)) }
  const submitRecord = async () => {
    const values = await recordForm.validateFields()
    if (editing && !isSafelyEditable(editing)) return
    putRecord.mutate({ existing: editing ?? undefined, desired: recordDesiredFromForm(values) })
  }
  const aliasEnabled = Form.useWatch('aliasEnabled', recordForm)
  const routingKind = Form.useWatch('routingKind', recordForm)
  const geoKind = Form.useWatch('geoKind', recordForm)
  const selectedApex = zone?.apex
  const statusAlert = result === null ? null : <Alert showIcon style={{ marginBottom: 16 }} type={result === 'applied' ? 'success' : result === 'pending' || result === 'ambiguous' ? 'warning' : 'error'} message={t(`cloud.route53.result.${result}`)} />
  const lifecycleSafe = lifecycle?.nonDefaultRecordCount === 0 && lifecycle?.dnssec.state === 'disabled'

  return <div>
    <PageHeader title={t('cloud.route53.title')} subtitle={t('cloud.route53.subtitle')} actions={<Button icon={<ReloadOutlined />} onClick={() => { invalidate(); setResult(null) }}>{t('btn.refresh')}</Button>} />
    {!canRead && <Alert type="warning" showIcon message={t('cloud.route53.permissionDenied')} />}
    {canRead && !canAccounts && <Alert type="warning" showIcon message={t('cloud.route53.accountDenied')} />}
    {canDnsAccess && <Space direction="vertical" size={16} style={{ width: '100%' }}>
      {statusAlert}
      <Space wrap><Typography.Text>{t('cloud.route53.account')}</Typography.Text><Select data-testid="route53-account" style={{ minWidth: 260 }} value={accountId} onChange={(value) => { setAccountId(value); setZone(undefined); setLifecycle(undefined); setResult(null) }} options={accounts.map((account) => ({ value: account.accountId, label: `${account.displayName} (${account.accountId})` }))} placeholder={t('cloud.route53.selectAccount')} /></Space>
      {!selected && accountId === undefined && <Alert type="info" showIcon message={t('cloud.route53.accountHint')} />}
      {selected && <Table size="small" rowKey="zoneId" loading={zones.isLoading} dataSource={zoneItems} title={() => <Space><Typography.Text strong>{t('cloud.route53.zones')}</Typography.Text>{canZonesWrite && <Button data-testid="route53-zone-create" size="small" icon={<PlusOutlined />} onClick={() => Modal.confirm({ title: t('cloud.route53.createZone'), content: <Form form={zoneForm} layout="vertical"><Form.Item name="apex" label={t('cloud.route53.apex')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { const values = await zoneForm.validateFields(); createZone.mutate(values.apex) }, okText: t('btn.create'), cancelText: t('btn.cancel') })}>{t('btn.create')}</Button>}{zones.hasNextPage && <Button size="small" loading={zones.isFetchingNextPage} onClick={() => zones.fetchNextPage()}>{t('cloud.route53.loadMore')}</Button>}</Space>} columns={[
        { title: t('cloud.route53.apex'), dataIndex: 'apex' }, { title: t('cloud.route53.visibility'), dataIndex: 'visibility' },
        { title: t('col.actions'), render: (_, item: Route53Zone) => <Space><Button size="small" onClick={() => { setZone(item); setLifecycle(undefined); setResult(null) }}>{t('cloud.route53.records')}</Button>{canZonesWrite && <Button size="small" onClick={() => inspectZone.mutate(item)} loading={inspectZone.isPending}>{t('cloud.route53.inspect')}</Button>}{canZonesWrite && <Button danger size="small" onClick={() => { deleteForm.resetFields(); Modal.confirm({ title: t('cloud.route53.deleteZone'), content: <Space direction="vertical"><Alert type="warning" showIcon message={t('cloud.route53.deleteZoneHint')} /><Form form={deleteForm} layout="vertical"><Form.Item name="confirmApex" label={t('cloud.route53.confirmApex', { apex: item.apex })} rules={[{ required: true }, { validator: (_, value) => value === item.apex ? Promise.resolve() : Promise.reject(new Error(t('cloud.route53.confirmMismatch'))) }]}><Input autoComplete="off" /></Form.Item></Form></Space>, onOk: async () => { await deleteForm.validateFields(); deleteZone.mutate(item) }, okText: t('btn.delete'), cancelText: t('btn.cancel') }) }}>{t('btn.delete')}</Button>}</Space> },
      ]} />}
      {lifecycle && <Descriptions size="small" bordered title={t('cloud.route53.lifecycle')} column={2}><Descriptions.Item label={t('cloud.route53.nameservers')}>{lifecycle.authoritativeNameservers.join(', ') || '—'}</Descriptions.Item><Descriptions.Item label={t('cloud.route53.delegation')}>{lifecycle.delegation.state}</Descriptions.Item><Descriptions.Item label={t('cloud.route53.readiness')}>{lifecycle.readiness}</Descriptions.Item><Descriptions.Item label={t('cloud.route53.dnssec')}>{lifecycle.dnssec.state}</Descriptions.Item><Descriptions.Item label={t('cloud.route53.nonDefaultRecords')}>{lifecycle.nonDefaultRecordCount}</Descriptions.Item><Descriptions.Item label={t('cloud.route53.deleteReadiness')}><Tag color={lifecycleSafe ? 'green' : 'gold'}>{t(lifecycleSafe ? 'cloud.route53.deleteReady' : 'cloud.route53.deleteBlocked')}</Tag></Descriptions.Item></Descriptions>}
      {zone && <Table size="small" rowKey={(record) => `${record.recordSet.key.owner}/${record.recordSet.key.recordType}/${record.recordSet.key.routing.type === 'route53' ? record.recordSet.key.routing.set_identifier : ''}`} loading={records.isLoading} dataSource={recordItems} title={() => <Space><Typography.Text strong>{t('cloud.route53.recordsFor', { zone: selectedApex ?? '' })}</Typography.Text>{canWrite && <Button data-testid="route53-record-create" type="primary" size="small" icon={<PlusOutlined />} onClick={() => openEditor(null)}>{t('cloud.route53.createRecord')}</Button>}{records.hasNextPage && <Button size="small" loading={records.isFetchingNextPage} onClick={() => records.fetchNextPage()}>{t('cloud.route53.loadMore')}</Button>}</Space>} columns={[
        { title: t('cloud.route53.owner'), render: (_, record: Route53RecordSet) => record.recordSet.key.owner }, { title: t('cloud.route53.recordType'), render: (_, record: Route53RecordSet) => record.recordSet.key.recordType },
        { title: t('cloud.route53.values'), render: (_, record: Route53RecordSet) => record.recordSet.values.map(recordValueText).join('; ') || t('cloud.route53.aliasValue') },
        { title: t('cloud.route53.routing'), render: (_, record: Route53RecordSet) => record.recordSet.extension?.routing_policy?.type ?? '—' }, { title: t('cloud.route53.healthCheck'), render: (_, record: Route53RecordSet) => record.recordSet.extension?.health_check_id ?? '—' },
        { title: t('cloud.route53.control'), render: () => <Tag>{t('cloud.route53.externalOrManual')}</Tag> },
        { title: t('col.actions'), render: (_, record: Route53RecordSet) => <Space>{canWrite && isSafelyEditable(record) && <Button size="small" onClick={() => openEditor(record)}>{t('btn.edit')}</Button>}{canWrite && record.recordSet.key.recordType !== 'SOA' && <Popconfirm title={t('cloud.route53.deleteRecordConfirm', { name: `${record.recordSet.key.owner} ${record.recordSet.key.recordType}` })} onConfirm={() => deleteRecord.mutate(record)} okText={t('btn.delete')} cancelText={t('btn.cancel')}><Button danger size="small">{t('btn.delete')}</Button></Popconfirm>}</Space> },
      ]} />}
      {zone === undefined && selected && !zones.isLoading && zoneItems.length === 0 && <Empty description={t('cloud.route53.noZones')} />}
    </Space>}
    <Modal title={t(editing ? 'cloud.route53.editRecord' : 'cloud.route53.createRecord')} open={editing !== undefined} onCancel={() => setEditing(undefined)} onOk={submitRecord} confirmLoading={putRecord.isPending} okText={t('btn.save')} cancelText={t('btn.cancel')} destroyOnClose>
      {editing && !isSafelyEditable(editing) && <Alert type="info" showIcon message={t('cloud.route53.readOnlyRecord')} />}
      <Form form={recordForm} layout="vertical" preserve={false}>
        <Form.Item name="owner" label={t('cloud.route53.owner')} rules={[{ required: true }]}><Input disabled={editing !== null && editing !== undefined} autoComplete="off" /></Form.Item>
        <Form.Item name="recordType" label={t('cloud.route53.recordType')} rules={[{ required: true }]}><Select disabled={editing !== null && editing !== undefined} options={EDITABLE_TYPES.map((value) => ({ value, label: value }))} /></Form.Item>
        <Form.Item name="setIdentifier" label={t('cloud.route53.setIdentifier')}><Input disabled={editing !== null && editing !== undefined} autoComplete="off" /></Form.Item>
        <Form.Item name="aliasEnabled" label={t('cloud.route53.aliasEnabled')} valuePropName="checked"><Switch /></Form.Item>
        {aliasEnabled && <><Form.Item name="aliasTargetZoneId" label={t('cloud.route53.aliasTargetZoneId')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="aliasTarget" label={t('cloud.route53.aliasTarget')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="evaluateTargetHealth" label={t('cloud.route53.evaluateTargetHealth')} valuePropName="checked"><Switch /></Form.Item></>}
        {!aliasEnabled && <><Form.Item name="value" label={t('cloud.route53.value')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="ttlSeconds" label={t('cloud.route53.ttlSeconds')} rules={[{ required: true }]}><Input type="number" min={0} /></Form.Item></>}
        <Form.Item name="routingKind" label={t('cloud.route53.routing')}><Select options={['none', 'weighted', 'failover', 'latency', 'geolocation', 'multivalue'].map((value) => ({ value, label: t(`cloud.route53.routing.${value}`) }))} /></Form.Item>
        {routingKind === 'weighted' && <Form.Item name="routingWeight" label={t('cloud.route53.routingWeight')} rules={[{ required: true }]}><Input type="number" min={0} max={255} /></Form.Item>}
        {routingKind === 'failover' && <Form.Item name="failoverRole" label={t('cloud.route53.failoverRole')} rules={[{ required: true }]}><Select options={['primary', 'secondary'].map((value) => ({ value, label: t(`cloud.route53.failover.${value}`) }))} /></Form.Item>}
        {routingKind === 'latency' && <Form.Item name="latencyRegion" label={t('cloud.route53.latencyRegion')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item>}
        {routingKind === 'geolocation' && <><Form.Item name="geoKind" label={t('cloud.route53.geoKind')} rules={[{ required: true }]}><Select options={['default', 'continent', 'country', 'us_subdivision'].map((value) => ({ value, label: t(`cloud.route53.geo.${value}`) }))} /></Form.Item><Form.Item name="geoCode" label={t('cloud.route53.geoCode')} rules={[{ required: geoKind !== 'default' }]}><Input autoComplete="off" /></Form.Item></>}
        <Form.Item name="healthCheckId" label={t('cloud.route53.healthCheck')}><Input autoComplete="off" /></Form.Item>
      </Form>
    </Modal>
  </div>
}
