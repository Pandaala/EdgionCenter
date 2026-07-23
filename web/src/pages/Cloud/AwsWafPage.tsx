import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, Button, Descriptions, Empty, Form, Input, InputNumber, Modal, Select, Space, Table, Tag, Typography } from 'antd'
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import { cloudApi } from '@/api/cloud'
import { cloudfrontApi } from '@/api/cloudfront'
import { awsWafApi, awsWafMutationResult, isValidAwsRegion, type AwsWafAction, type AwsWafAssociation, type AwsWafIpSet, type AwsWafRule, type AwsWafRuleWrite, type AwsWafScope, type AwsWafStatement, type AwsWafVisibility, type AwsWafWebAclSummary } from '@/api/awsWaf'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import { useCan } from '@/utils/permissions'

const KEY = ['aws-waf']
type Result = 'accepted' | 'conflicted' | 'ambiguous' | 'rejected' | null
export type RuleValues = { reference: string; name: string; priority: number; action?: AwsWafAction; kind: AwsWafStatement['kind']; vendorName?: string; managedName?: string; version?: string; excludedRules?: string; arn?: string; rateLimit?: number; metricName: string }
type AclValues = { name: string; defaultAction: 'allow' | 'block'; metricName: string }
type IpSetValues = { name: string; addressVersion: 'ipv4' | 'ipv6'; addresses: string }

const visibility = (metricName: string): AwsWafVisibility => ({ metricName, cloudwatchMetricsEnabled: true, sampledRequestsEnabled: true })
export function statementFromForm(values: RuleValues, existing?: AwsWafRule): AwsWafStatement {
  if (values.kind === 'managed_rule_group') {
    const existingOverrides = existing?.statement.kind === 'managed_rule_group' ? existing.statement.ruleActionOverrides : []
    return { kind: values.kind, vendorName: values.vendorName!, name: values.managedName!, version: values.version || undefined, excludedRules: values.excludedRules?.split(',').map((value) => value.trim()).filter(Boolean) ?? [], ruleActionOverrides: existingOverrides }
  }
  if (values.kind === 'ip_set_reference') return { kind: values.kind, arn: values.arn! }
  return { kind: 'rate_based', limit: Number(values.rateLimit), scopeDownIpSet: values.arn ? { arn: values.arn } : undefined }
}
export function ruleFromForm(values: RuleValues, lockToken: string, existing?: AwsWafRule): AwsWafRuleWrite {
  const statement = statementFromForm(values, existing)
  const base = { reference: values.reference, lockToken, name: values.name, priority: Number(values.priority), statement, visibility: visibility(values.metricName) }
  if (statement.kind === 'managed_rule_group') return { ...base, managedOverrideAction: 'none' }
  if (!values.action) throw new Error('action_required')
  return { ...base, action: values.action }
}

export default function AwsWafPage({ writeAvailable = false, attachAvailable = false, detachAvailable = false, securityWeakenAvailable = false, cloudfrontWriteAvailable = false }: { writeAvailable?: boolean; attachAvailable?: boolean; detachAvailable?: boolean; securityWeakenAvailable?: boolean; cloudfrontWriteAvailable?: boolean }) {
  const t = useT()
  const client = useQueryClient()
  const canRead = useCan('aws-waf:read')
  const canAccounts = useCan('provider-accounts:read')
  const canAttach = useCan('aws-waf:attach') && attachAvailable
  const canDetach = useCan('aws-waf:detach') && detachAvailable
  const canWrite = useCan('aws-waf:write') && writeAvailable
  const canException = useCan('aws-waf:exception') && writeAvailable
  const canWeaken = useCan('aws-waf:security-weaken') && securityWeakenAvailable
  const canCloudfrontAttach = canAttach && cloudfrontWriteAvailable
  const [accountId, setAccountId] = useState<string>()
  const [scopeKind, setScopeKind] = useState<'cloudfront' | 'regional'>('cloudfront')
  const [region, setRegion] = useState('us-east-1')
  const [selectedAcl, setSelectedAcl] = useState<string>()
  const [result, setResult] = useState<Result>(null)
  const [refreshRequired, setRefreshRequired] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [capacityPreview, setCapacityPreview] = useState<{ requiredWcu: number; allowed: boolean; reason: string }>()
  const [attachForm] = Form.useForm<{ distributionId: string }>()
  const [detachForm] = Form.useForm<{ resourceArn: string; resourceKind: AwsWafAssociation['resourceKind']; confirmation: string }>()
  const [weakenForm] = Form.useForm<{ action?: 'allow' | 'count'; confirmation: string }>()
  const [exceptionForm] = Form.useForm<{ excludedRules: string; confirmation: string }>()
  const [aclForm] = Form.useForm<AclValues>()
  const [ruleForm] = Form.useForm<RuleValues>()
  const [ipSetForm] = Form.useForm<IpSetValues>()
  const [attachRegionalForm] = Form.useForm<{ resourceArn: string; resourceKind: AwsWafAssociation['resourceKind'] }>()
  const scope: AwsWafScope = scopeKind === 'cloudfront' ? { type: 'cloudfront' } : { type: 'regional', region }
  const hasValidScope = scope.type === 'cloudfront' || isValidAwsRegion(region)
  const canAccess = canRead && canAccounts && accountId !== undefined && hasValidScope
  const accountsQuery = useQuery({ queryKey: ['cloud-provider-accounts'], queryFn: cloudApi.listAccounts, enabled: canAccounts })
  const accounts = useMemo(() => (accountsQuery.data?.data ?? []).filter((item) => item.provider === 'aws'), [accountsQuery.data?.data])
  const acls = useQuery({ queryKey: [...KEY, accountId, scope.type, scope.type === 'regional' ? scope.region : 'global', 'acls'], queryFn: () => awsWafApi.listWebAcls(accountId!, scope), enabled: canAccess })
  const ipSets = useQuery({ queryKey: [...KEY, accountId, scope.type, scope.type === 'regional' ? scope.region : 'global', 'ipsets'], queryFn: () => awsWafApi.listIpSets(accountId!, scope), enabled: canAccess })
  const catalog = useQuery({ queryKey: [...KEY, accountId, scope.type, scope.type === 'regional' ? scope.region : 'global', 'catalog'], queryFn: () => awsWafApi.listCatalog(accountId!, scope), enabled: canAccess })
  const detail = useQuery({ queryKey: [...KEY, accountId, scope.type, scope.type === 'regional' ? scope.region : 'global', selectedAcl], queryFn: () => awsWafApi.getWebAcl(accountId!, scope, selectedAcl!), enabled: canAccess && selectedAcl !== undefined })
  const associations = useQuery({ queryKey: [...KEY, accountId, scope.type, scope.type === 'regional' ? scope.region : 'global', selectedAcl, 'associations'], queryFn: () => awsWafApi.associations(accountId!, scope, selectedAcl!), enabled: canAccess && selectedAcl !== undefined })
  const distributions = useQuery({ queryKey: ['cloudfront-distributions', accountId], queryFn: () => cloudfrontApi.list(accountId!), enabled: canAccess && scope.type === 'cloudfront' })
  const fail = (error: unknown) => { const next = awsWafMutationResult(error); setResult(next); if (next === 'ambiguous') setRefreshRequired(true) }
  const invalidate = () => client.invalidateQueries({ queryKey: KEY })
  const weaken = useMutation({ mutationFn: ({ rule, action }: { rule: AwsWafRule; action?: 'allow' | 'count' }) => {
    const confirmation = `${selectedAcl}/${rule.reference}`
    const request = rule.statement.kind === 'managed_rule_group'
      ? { lockToken: detail.data!.data!.lockToken, managedOverrideAction: 'count' as const, confirmation }
      : { lockToken: detail.data!.data!.lockToken, action: action!, confirmation }
    return awsWafApi.securityWeakenRule(accountId!, scope, selectedAcl!, rule.reference!, request)
  }, onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const attach = useMutation({ mutationFn: (distributionId: string) => cloudfrontApi.setWebAcl(accountId!, distributionId, { webAclId: selectedAcl! }), onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const detach = useMutation({ mutationFn: (values: { resourceArn: string; resourceKind: AwsWafAssociation['resourceKind']; confirmation: string }) => { if (scope.type !== 'regional') throw new Error('regional_scope_required'); return awsWafApi.detachRegional(accountId!, scope, values) }, onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const createAcl = useMutation({ mutationFn: (values: AclValues) => awsWafApi.createWebAcl(accountId!, scope, { name: values.name, defaultAction: values.defaultAction, visibility: visibility(values.metricName) }), onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const updateAcl = useMutation({ mutationFn: (metricName: string) => awsWafApi.updateWebAcl(accountId!, scope, selectedAcl!, { lockToken: selected!.lockToken, visibility: visibility(metricName) }), onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const saveRule = useMutation({ mutationFn: ({ values, existing }: { values: RuleValues; existing?: AwsWafRule }) => { const request = ruleFromForm(values, selected!.lockToken, existing); return existing ? awsWafApi.updateRule(accountId!, scope, selectedAcl!, existing.reference!, request) : awsWafApi.createRule(accountId!, scope, selectedAcl!, request) }, onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const removeRule = useMutation({ mutationFn: (rule: AwsWafRule) => awsWafApi.deleteRule(accountId!, scope, selectedAcl!, rule.reference!, { lockToken: selected!.lockToken, confirmation: `${selectedAcl}/${rule.reference}` }), onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const exception = useMutation({ mutationFn: ({ rule, excludedRules }: { rule: AwsWafRule; excludedRules: string[] }) => awsWafApi.managedException(accountId!, scope, selectedAcl!, rule.reference!, { lockToken: selected!.lockToken, excludedRules, confirmation: `${selectedAcl}/${rule.reference}` }), onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const saveIpSet = useMutation({ mutationFn: ({ values, existing }: { values: IpSetValues; existing?: AwsWafIpSet }) => { const addresses = values.addresses.split(',').map((value) => value.trim()).filter(Boolean); return existing ? awsWafApi.updateIpSet(accountId!, scope, existing.id, { lockToken: existing.lockToken, addresses }) : awsWafApi.createIpSet(accountId!, scope, { name: values.name, addressVersion: values.addressVersion, addresses }) }, onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const removeIpSet = useMutation({ mutationFn: (ipSet: AwsWafIpSet) => awsWafApi.deleteIpSet(accountId!, scope, ipSet.id, { lockToken: ipSet.lockToken, confirmation: ipSet.id }), onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const attachRegional = useMutation({ mutationFn: (values: { resourceArn: string; resourceKind: AwsWafAssociation['resourceKind'] }) => { if (scope.type !== 'regional') throw new Error('regional_scope_required'); return awsWafApi.attachRegional(accountId!, scope, selectedAcl!, values) }, onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const weakenAcl = useMutation({ mutationFn: () => awsWafApi.weakenWebAcl(accountId!, scope, selectedAcl!, { lockToken: selected!.lockToken, defaultAction: 'allow', confirmation: selectedAcl! }), onSuccess: () => { setResult('accepted'); invalidate() }, onError: fail })
  const removeAcl = useMutation({ mutationFn: () => awsWafApi.deleteWebAcl(accountId!, scope, selectedAcl!, { lockToken: selected!.lockToken, confirmation: selectedAcl! }), onSuccess: () => { setSelectedAcl(undefined); setResult('accepted'); invalidate() }, onError: fail })
  const busy = weaken.isPending || attach.isPending || detach.isPending || createAcl.isPending || updateAcl.isPending || saveRule.isPending || removeRule.isPending || exception.isPending || saveIpSet.isPending || removeIpSet.isPending || attachRegional.isPending || weakenAcl.isPending || removeAcl.isPending
  const mutationDisabled = busy || refreshRequired
  const aclItems = acls.data?.data ?? []
  const selected = detail.data?.data
  const resultAlert = result === null ? null : <Alert type={result === 'accepted' ? 'success' : result === 'ambiguous' ? 'warning' : 'error'} showIcon message={t(`cloud.awsWaf.result.${result}`)} />
  const resetScope = () => { setSelectedAcl(undefined) }
  const refresh = async () => {
    if (!canAccess) return
    setRefreshing(true)
    try {
      const results = selectedAcl === undefined
        ? await Promise.all([acls.refetch(), ipSets.refetch(), catalog.refetch()])
        : await Promise.all([acls.refetch(), ipSets.refetch(), catalog.refetch(), detail.refetch(), associations.refetch()])
      if (results.every((item) => item.isSuccess)) {
        setRefreshRequired(false)
        setResult(null)
      }
    } finally {
      setRefreshing(false)
    }
  }
  const openAclEditor = () => {
    aclForm.setFieldsValue({ defaultAction: 'block', metricName: 'edgion-center-waf' })
    Modal.confirm({ title: t('cloud.awsWaf.createAcl'), content: <Form form={aclForm} layout="vertical"><Form.Item name="name" label={t('cloud.awsWaf.name')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="defaultAction" label={t('cloud.awsWaf.defaultAction')}><Select options={[{ value: 'block', label: t('cloud.awsWaf.action.block') }, { value: 'allow', label: t('cloud.awsWaf.action.allow') }]} /></Form.Item><Form.Item name="metricName" label={t('cloud.awsWaf.metricName')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => createAcl.mutate(await aclForm.validateFields()), okText: t('btn.create'), cancelText: t('btn.cancel') })
  }
  const openAclVisibilityEditor = () => {
    aclForm.setFieldsValue({ metricName: selected!.visibility.metricName })
    Modal.confirm({ title: t('cloud.awsWaf.editAcl'), content: <Form form={aclForm} layout="vertical"><Form.Item name="metricName" label={t('cloud.awsWaf.metricName')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { const values = await aclForm.validateFields(); updateAcl.mutate(values.metricName) }, okText: t('btn.save'), cancelText: t('btn.cancel') })
  }
  const openRuleEditor = (existing?: AwsWafRule) => {
    const statement = existing?.statement
    ruleForm.setFieldsValue({ reference: existing?.reference, name: existing?.name, priority: existing?.priority ?? 0, action: existing?.action ?? 'block', kind: statement?.kind ?? 'managed_rule_group', vendorName: statement?.kind === 'managed_rule_group' ? statement.vendorName : 'AWS', managedName: statement?.kind === 'managed_rule_group' ? statement.name : undefined, version: statement?.kind === 'managed_rule_group' ? statement.version : undefined, excludedRules: statement?.kind === 'managed_rule_group' ? statement.excludedRules.join(', ') : undefined, arn: statement?.kind === 'ip_set_reference' ? statement.arn : statement?.kind === 'rate_based' ? statement.scopeDownIpSet?.arn : undefined, rateLimit: statement?.kind === 'rate_based' ? statement.limit : 100, metricName: existing?.visibility.metricName ?? 'edgion-center-rule' })
    Modal.confirm({ title: t(existing ? 'cloud.awsWaf.editRule' : 'cloud.awsWaf.createRule'), content: <Form form={ruleForm} layout="vertical"><Form.Item name="reference" label={t('cloud.awsWaf.reference')} rules={[{ required: true }]}><Input disabled={existing !== undefined} autoComplete="off" /></Form.Item><Form.Item name="name" label={t('cloud.awsWaf.name')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="priority" label={t('cloud.awsWaf.priority')} rules={[{ required: true }]}><InputNumber min={0} /></Form.Item><Form.Item name="kind" label={t('cloud.awsWaf.ruleType')} rules={[{ required: true }]}><Select disabled={existing !== undefined} options={[{ value: 'managed_rule_group', label: t('cloud.awsWaf.ruleType.managed') }, { value: 'ip_set_reference', label: t('cloud.awsWaf.ruleType.ipSet') }, { value: 'rate_based', label: t('cloud.awsWaf.ruleType.rate') }]} /></Form.Item><Form.Item noStyle shouldUpdate={(previous, current) => previous.kind !== current.kind}>{({ getFieldValue }) => getFieldValue('kind') !== 'managed_rule_group' && <Form.Item name="action" label={t('cloud.awsWaf.action')} rules={[{ required: true }]}><Select options={['block', 'allow', 'count', 'challenge', 'captcha'].map((value) => ({ value, label: t(`cloud.awsWaf.action.${value}`) }))} /></Form.Item>}</Form.Item><Form.Item name="vendorName" label={t('cloud.awsWaf.vendor')}><Input autoComplete="off" /></Form.Item><Form.Item name="managedName" label={t('cloud.awsWaf.managedName')}><Input autoComplete="off" /></Form.Item><Form.Item name="version" label={t('cloud.awsWaf.version')}><Input autoComplete="off" /></Form.Item><Form.Item name="excludedRules" label={t('cloud.awsWaf.excludedRules')}><Input autoComplete="off" /></Form.Item><Form.Item name="arn" label={t('cloud.awsWaf.ipSetArn')}><Input autoComplete="off" /></Form.Item><Form.Item name="rateLimit" label={t('cloud.awsWaf.rateLimit')}><InputNumber min={100} /></Form.Item><Form.Item name="metricName" label={t('cloud.awsWaf.metricName')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { const values = await ruleForm.validateFields(); const candidate = ruleFromForm(values, selected!.lockToken, existing); const preview = (await awsWafApi.capacity(accountId!, scope, [candidate])).data; setCapacityPreview(preview); if (!preview?.allowed) throw new Error(preview?.reason ?? 'capacity_rejected'); saveRule.mutate({ values, existing }) }, okText: t('btn.save'), cancelText: t('btn.cancel') })
  }
  const openIpSetEditor = (existing?: AwsWafIpSet) => {
    ipSetForm.setFieldsValue({ name: existing?.name, addressVersion: existing?.addressVersion ?? 'ipv4', addresses: existing?.addresses.join(', ') })
    Modal.confirm({ title: t(existing ? 'cloud.awsWaf.editIpSet' : 'cloud.awsWaf.createIpSet'), content: <Form form={ipSetForm} layout="vertical"><Form.Item name="name" label={t('cloud.awsWaf.name')} rules={[{ required: true }]}><Input disabled={existing !== undefined} autoComplete="off" /></Form.Item><Form.Item name="addressVersion" label={t('cloud.awsWaf.addressVersion')} rules={[{ required: true }]}><Select disabled={existing !== undefined} options={[{ value: 'ipv4', label: 'IPv4' }, { value: 'ipv6', label: 'IPv6' }]} /></Form.Item><Form.Item name="addresses" label={t('cloud.awsWaf.addresses')} rules={[{ required: true }]}><Input.TextArea autoComplete="off" /></Form.Item></Form>, onOk: async () => saveIpSet.mutate({ values: await ipSetForm.validateFields(), existing }), okText: existing ? t('btn.save') : t('btn.create'), cancelText: t('btn.cancel') })
  }

  return <div>
    <PageHeader title={t('cloud.awsWaf.title')} subtitle={t('cloud.awsWaf.subtitle')} actions={<Button icon={<ReloadOutlined />} disabled={refreshing || !canAccess} onClick={refresh}>{t('btn.refresh')}</Button>} />
    {!canRead && <Alert type="warning" showIcon message={t('cloud.awsWaf.permissionDenied')} />}
    {canRead && !canAccounts && <Alert type="warning" showIcon message={t('cloud.awsWaf.accountDenied')} />}
    {canRead && canAccounts && <Space direction="vertical" size={16} style={{ width: '100%', marginTop: 16 }}>
      {resultAlert}
      {refreshRequired && <Alert type="warning" showIcon message={t('cloud.awsWaf.refreshRequired')} />}
      <Space wrap><Typography.Text>{t('cloud.awsWaf.account')}</Typography.Text><Select data-testid="aws-waf-account" style={{ minWidth: 240 }} value={accountId} onChange={(value) => { setAccountId(value); resetScope() }} options={accounts.map((item) => ({ value: item.accountId, label: `${item.displayName} (${item.accountId})` }))} placeholder={t('cloud.awsWaf.selectAccount')} /><Typography.Text>{t('cloud.awsWaf.scope')}</Typography.Text><Select data-testid="aws-waf-scope" style={{ minWidth: 160 }} value={scopeKind} onChange={(value) => { setScopeKind(value); resetScope() }} options={[{ value: 'cloudfront', label: t('cloud.awsWaf.scope.cloudfront') }, { value: 'regional', label: t('cloud.awsWaf.scope.regional') }]} />{scopeKind === 'regional' && <Input data-testid="aws-waf-region" value={region} onChange={(event) => { setRegion(event.target.value); resetScope() }} placeholder={t('cloud.awsWaf.region')} style={{ width: 160 }} />}</Space>
      {accountId !== undefined && scope.type === 'regional' && !hasValidScope && <Alert type="warning" showIcon message={t('cloud.awsWaf.invalidRegion')} />}
      {accountId === undefined && <Alert type="info" showIcon message={t('cloud.awsWaf.accountHint')} />}
      {capacityPreview && <Alert type={capacityPreview.allowed ? 'info' : 'warning'} showIcon message={t('cloud.awsWaf.capacityPreview', { wcu: capacityPreview.requiredWcu, reason: capacityPreview.reason })} />}
      {accountId !== undefined && <Table data-testid="aws-waf-acls" size="small" rowKey="id" loading={acls.isLoading} dataSource={aclItems} title={() => <Space><Typography.Text strong>{t('cloud.awsWaf.webAcls')}</Typography.Text>{canWrite && <Button size="small" icon={<PlusOutlined />} disabled={mutationDisabled} onClick={openAclEditor}>{t('btn.create')}</Button>}</Space>} columns={[
        { title: t('cloud.awsWaf.name'), dataIndex: 'name' }, { title: t('cloud.awsWaf.capacity'), dataIndex: 'capacity' }, { title: t('cloud.awsWaf.lock'), render: (_: unknown, item: AwsWafWebAclSummary) => <Tag color={item.lockTokenPresent ? 'green' : 'gold'}>{t(item.lockTokenPresent ? 'cloud.awsWaf.lock.present' : 'cloud.awsWaf.lock.absent')}</Tag> },
        { title: t('col.actions'), render: (_: unknown, item: AwsWafWebAclSummary) => <Button size="small" onClick={() => setSelectedAcl(item.id)}>{t('btn.view')}</Button> },
      ]} />}
      {accountId !== undefined && !acls.isLoading && aclItems.length === 0 && <Empty description={t('cloud.awsWaf.noWebAcls')} />}
      {selected && <Descriptions title={t('cloud.awsWaf.detail')} bordered size="small" column={2}><Descriptions.Item label={t('cloud.awsWaf.name')}>{selected.name}</Descriptions.Item><Descriptions.Item label={t('cloud.awsWaf.capacity')}>{selected.capacity}</Descriptions.Item><Descriptions.Item label={t('cloud.awsWaf.defaultAction')}>{selected.defaultAction}</Descriptions.Item><Descriptions.Item label={t('cloud.awsWaf.lock')}><Tag color="green">{t('cloud.awsWaf.lock.present')}</Tag></Descriptions.Item></Descriptions>}
      {selected && <Space>{canWrite && <Button disabled={busy || refreshRequired} onClick={openAclVisibilityEditor}>{t('btn.edit')}</Button>}{canWeaken && <><Button danger disabled={busy || refreshRequired} onClick={() => { weakenForm.resetFields(); Modal.confirm({ title: t('cloud.awsWaf.weakenDefault'), content: <Form form={weakenForm} layout="vertical"><Form.Item name="confirmation" label={t('cloud.awsWaf.confirmTarget', { target: selected.id })} rules={[{ required: true }, { validator: (_, value) => value === selected.id ? Promise.resolve() : Promise.reject(new Error(t('cloud.awsWaf.confirmMismatch'))) }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { await weakenForm.validateFields(); weakenAcl.mutate() }, okText: t('btn.confirm'), cancelText: t('btn.cancel') }) }}>{t('cloud.awsWaf.weakenDefault')}</Button><Button danger disabled={busy || refreshRequired} onClick={() => { weakenForm.resetFields(); Modal.confirm({ title: t('cloud.awsWaf.deleteAcl'), content: <Form form={weakenForm} layout="vertical"><Form.Item name="confirmation" label={t('cloud.awsWaf.confirmTarget', { target: selected.id })} rules={[{ required: true }, { validator: (_, value) => value === selected.id ? Promise.resolve() : Promise.reject(new Error(t('cloud.awsWaf.confirmMismatch'))) }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { await weakenForm.validateFields(); removeAcl.mutate() }, okText: t('btn.delete'), cancelText: t('btn.cancel') }) }}>{t('cloud.awsWaf.deleteAcl')}</Button></>}</Space>}
      {selected && <Table size="small" rowKey={(item) => item.reference ?? item.name} dataSource={selected.rules} title={() => <Space><Typography.Text strong>{t('cloud.awsWaf.rules')}</Typography.Text>{canWrite && <Button size="small" icon={<PlusOutlined />} disabled={busy || refreshRequired} onClick={() => openRuleEditor()}>{t('btn.create')}</Button>}</Space>} columns={[
        { title: t('cloud.awsWaf.name'), dataIndex: 'name' }, { title: t('cloud.awsWaf.priority'), dataIndex: 'priority' }, { title: t('cloud.awsWaf.action'), render: (_: unknown, item: AwsWafRule) => item.statement.kind === 'managed_rule_group' ? item.managedOverrideAction ?? '-' : item.action ?? '-' }, { title: t('cloud.awsWaf.ownership'), dataIndex: 'ownership' },
        { title: t('col.actions'), render: (_: unknown, item: AwsWafRule) => {
          const target = `${selectedAcl}/${item.reference}`
          const confirmationRule = { required: true, validator: (_: unknown, value: string) => value === target ? Promise.resolve() : Promise.reject(new Error(t('cloud.awsWaf.confirmMismatch'))) }
          const managed = item.statement.kind === 'managed_rule_group'
          const managedCount = item.managedOverrideAction === 'count'
          return <Space>{canWrite && item.reference && !managedCount && <Button size="small" disabled={busy || refreshRequired} onClick={() => openRuleEditor(item)}>{t('btn.edit')}</Button>}{canException && item.reference && managed && <Button size="small" disabled={busy || refreshRequired} onClick={() => { exceptionForm.setFieldsValue({ excludedRules: item.statement.kind === 'managed_rule_group' ? item.statement.excludedRules.join(', ') : '' }); Modal.confirm({ title: t('cloud.awsWaf.exception'), content: <Form form={exceptionForm} layout="vertical"><Form.Item name="excludedRules" label={t('cloud.awsWaf.excludedRules')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="confirmation" label={t('cloud.awsWaf.confirmTarget', { target })} rules={[confirmationRule]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { const values = await exceptionForm.validateFields(); exception.mutate({ rule: item, excludedRules: values.excludedRules.split(',').map((value) => value.trim()).filter(Boolean) }) }, okText: t('btn.save'), cancelText: t('btn.cancel') }) }}>{t('cloud.awsWaf.exception')}</Button>}{canWeaken && item.reference && !managedCount && <Button danger size="small" disabled={busy || refreshRequired} onClick={() => { weakenForm.resetFields(); Modal.confirm({ title: t('cloud.awsWaf.weaken'), content: <Form form={weakenForm} layout="vertical">{!managed && <Form.Item name="action" label={t('cloud.awsWaf.action')} initialValue="count" rules={[{ required: true }]}><Select options={[{ value: 'count', label: t('cloud.awsWaf.action.count') }, { value: 'allow', label: t('cloud.awsWaf.action.allow') }]} /></Form.Item>}<Form.Item name="confirmation" label={t('cloud.awsWaf.confirmTarget', { target })} rules={[confirmationRule]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { const values = await weakenForm.validateFields(); weaken.mutate({ rule: item, action: values.action }) }, okText: t('btn.confirm'), cancelText: t('btn.cancel') }) }}>{t('cloud.awsWaf.weaken')}</Button>}{canWeaken && item.reference && <Button danger size="small" disabled={busy || refreshRequired} onClick={() => { weakenForm.resetFields(); Modal.confirm({ title: t('cloud.awsWaf.deleteRule'), content: <Form form={weakenForm} layout="vertical"><Form.Item name="confirmation" label={t('cloud.awsWaf.confirmTarget', { target })} rules={[confirmationRule]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { await weakenForm.validateFields(); removeRule.mutate(item) }, okText: t('btn.delete'), cancelText: t('btn.cancel') }) }}>{t('btn.delete')}</Button>}</Space>
        } },
      ]} />}
      {selected && <Table size="small" rowKey="resourceArn" dataSource={associations.data?.data ?? []} title={() => <Space><Typography.Text strong>{t('cloud.awsWaf.associations')}</Typography.Text>{scope.type === 'regional' && canAttach && <Button size="small" disabled={mutationDisabled} onClick={() => { attachRegionalForm.resetFields(); Modal.confirm({ title: t('cloud.awsWaf.attachRegional'), content: <Form form={attachRegionalForm} layout="vertical"><Form.Item name="resourceArn" label={t('cloud.awsWaf.target')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item><Form.Item name="resourceKind" label={t('cloud.awsWaf.targetKind')} rules={[{ required: true }]}><Select options={['application_load_balancer', 'api_gateway_stage', 'app_sync_api', 'cognito_user_pool'].map((value) => ({ value, label: t(`cloud.awsWaf.targetKind.${value}`) }))} /></Form.Item></Form>, onOk: async () => attachRegional.mutate(await attachRegionalForm.validateFields()), okText: t('btn.save'), cancelText: t('btn.cancel') }) }}>{t('cloud.awsWaf.attachRegional')}</Button>}</Space>} columns={[{ title: t('cloud.awsWaf.target'), dataIndex: 'resourceArn' }, { title: t('cloud.awsWaf.authority'), dataIndex: 'targetDeploymentAuthority' }, { title: t('col.actions'), render: (_: unknown, item: AwsWafAssociation) => scope.type === 'regional' && canDetach && <Button danger size="small" disabled={mutationDisabled} onClick={() => { detachForm.setFieldsValue({ resourceArn: item.resourceArn, resourceKind: item.resourceKind }); Modal.confirm({ title: t('cloud.awsWaf.detach'), content: <Form form={detachForm} layout="vertical"><Form.Item name="confirmation" label={t('cloud.awsWaf.confirmTarget', { target: item.resourceArn })} rules={[{ required: true }, { validator: (_, value) => value === item.resourceArn ? Promise.resolve() : Promise.reject(new Error(t('cloud.awsWaf.confirmMismatch'))) }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => detach.mutate(await detachForm.validateFields()), okText: t('btn.confirm'), cancelText: t('btn.cancel') }) }}>{t('cloud.awsWaf.detach')}</Button> }]} />}
      {selected && scope.type === 'cloudfront' && canCloudfrontAttach && <Button disabled={mutationDisabled} onClick={() => { attachForm.resetFields(); Modal.confirm({ title: t('cloud.awsWaf.attachCloudfront'), content: <Form form={attachForm} layout="vertical"><Form.Item name="distributionId" label={t('cloud.awsWaf.distribution')} rules={[{ required: true }]}><Select options={(distributions.data?.data ?? []).map((item) => ({ value: item.id, label: `${item.id} (${item.domainName})` }))} /></Form.Item></Form>, onOk: async () => { const values = await attachForm.validateFields(); attach.mutate(values.distributionId) }, okText: t('btn.confirm'), cancelText: t('btn.cancel') }) }}>{t('cloud.awsWaf.attachCloudfront')}</Button>}
      {accountId !== undefined && <><Table size="small" rowKey="id" dataSource={ipSets.data?.data ?? []} title={() => <Space><Typography.Text strong>{t('cloud.awsWaf.ipSets')}</Typography.Text>{canWrite && <Button size="small" icon={<PlusOutlined />} disabled={mutationDisabled} onClick={() => openIpSetEditor()}>{t('btn.create')}</Button>}</Space>} columns={[{ title: t('cloud.awsWaf.name'), dataIndex: 'name' }, { title: t('cloud.awsWaf.addresses'), render: (_: unknown, item: AwsWafIpSet) => item.addresses.length }, { title: t('cloud.awsWaf.lock'), render: (_: unknown, item: AwsWafIpSet) => <Tag color={item.lockToken ? 'green' : 'gold'}>{t(item.lockToken ? 'cloud.awsWaf.lock.present' : 'cloud.awsWaf.lock.absent')}</Tag> }, { title: t('col.actions'), render: (_: unknown, item: AwsWafIpSet) => (canWrite || canWeaken) && <Space>{canWrite && <Button size="small" disabled={mutationDisabled} onClick={() => openIpSetEditor(item)}>{t('btn.edit')}</Button>}{canWeaken && <Button danger size="small" disabled={mutationDisabled} onClick={() => { weakenForm.resetFields(); Modal.confirm({ title: t('cloud.awsWaf.deleteIpSet'), content: <Form form={weakenForm} layout="vertical"><Form.Item name="confirmation" label={t('cloud.awsWaf.confirmTarget', { target: item.id })} rules={[{ required: true }, { validator: (_, value) => value === item.id ? Promise.resolve() : Promise.reject(new Error(t('cloud.awsWaf.confirmMismatch'))) }]}><Input autoComplete="off" /></Form.Item></Form>, onOk: async () => { await weakenForm.validateFields(); removeIpSet.mutate(item) }, okText: t('btn.delete'), cancelText: t('btn.cancel') }) }}>{t('btn.delete')}</Button>}</Space> }]} /><Table size="small" rowKey={(item) => `${item.vendorName}/${item.name}`} dataSource={catalog.data?.data ?? []} title={() => t('cloud.awsWaf.managedCatalog')} columns={[{ title: t('cloud.awsWaf.vendor'), dataIndex: 'vendorName' }, { title: t('cloud.awsWaf.name'), dataIndex: 'name' }, { title: t('cloud.awsWaf.versions'), render: (_: unknown, item) => item.versions.join(', ') }]} /></>}
    </Space>}
  </div>
}
