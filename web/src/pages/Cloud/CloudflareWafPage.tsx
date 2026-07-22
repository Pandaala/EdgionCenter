import { useMemo, useState } from 'react'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, Button, Descriptions, Empty, Form, Input, InputNumber, Modal, Select, Space, Table, Tabs, Tag, Typography, message } from 'antd'
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import type { AxiosError } from 'axios'
import { cloudApi, type ProviderAccount } from '@/api/cloud'
import { cloudflareDnsApi, type CloudflareZone } from '@/api/cloudflareDns'
import { cloudflareWafApi, type CloudflareWafAction, type CloudflareWafDefinition, type CloudflareWafPhase, type CloudflareWafPosition, type CloudflareWafRule, type CloudflareWafRuleset, type WafRuleValues } from '@/api/cloudflareWaf'
import { useCan } from '@/utils/permissions'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'

const WAF_KEY = ['cloudflare-waf']
const CONFIRMATION = 'WEAKEN_CLOUDFLARE_WAF'
const PHASES: CloudflareWafPhase[] = ['managed', 'custom', 'rate_limit']
const ACTIONS: CloudflareWafAction[] = ['block', 'challenge', 'managed_challenge']

interface RuleFormValues extends WafRuleValues {
  managedRulesetIdsText?: string
  position?: number
  securityConfirmation?: string
}

type Editor = { ruleset: CloudflareWafRuleset; rule?: CloudflareWafRule; mode: 'create' | 'edit' | 'exception' } | null
type SecurityTarget = { ruleset: CloudflareWafRuleset; rule: CloudflareWafRule; mode: 'downgrade' | 'delete' } | null
type MutationResult = 'applied' | 'rejected' | 'conflicted' | 'ambiguous' | null

function selectedCloudflareAccount(accounts: ProviderAccount[], id: string | undefined): ProviderAccount | undefined {
  return accounts.find((account) => account.accountId === id && account.provider === 'cloudflare')
}

function errorResult(error: unknown): MutationResult {
  const response = (error as AxiosError<{ error?: string }>).response
  if (response?.data?.error === 'unknown_outcome') return 'ambiguous'
  if (response?.status === 409 || response?.status === 412) return 'conflicted'
  return 'rejected'
}

function definitionValues(definition: CloudflareWafDefinition | undefined): RuleFormValues {
  if (!definition) return { reference: '', description: '', expression: '', action: 'block', characteristics: ['ip_source'], periodSecs: 60, requestsPerPeriod: 100, mitigationTimeoutSecs: 60 }
  if (definition.kind === 'managed') return { ...definition, action: 'block' }
  if (definition.kind === 'managed_exception') return { ...definition, action: 'block', managedRulesetIdsText: definition.managedRulesetIds.join(', '), position: definition.position.type === 'index' ? definition.position.index : 1 }
  return { ...definition }
}

function position(values: RuleFormValues): CloudflareWafPosition | undefined {
  return values.position ? { type: 'index', index: Number(values.position) } : undefined
}

function standardValues(values: RuleFormValues): WafRuleValues {
  return {
    reference: values.reference,
    description: values.description,
    expression: values.expression,
    action: values.action,
    managedRulesetId: values.managedRulesetId,
    managedRulesetIds: values.managedRulesetIdsText?.split(',').map((value) => value.trim()).filter(Boolean),
    characteristics: values.characteristics,
    periodSecs: Number(values.periodSecs),
    requestsPerPeriod: Number(values.requestsPerPeriod),
    mitigationTimeoutSecs: Number(values.mitigationTimeoutSecs),
  }
}

function availabilityColor(value: string): string {
  if (value === 'available') return 'green'
  if (value === 'permission_denied' || value === 'quota_limited') return 'gold'
  return 'red'
}

function effectiveState(rule: CloudflareWafRule): 'preview' | 'enforced' | 'disabled' {
  if (!rule.enabled) return 'disabled'
  return rule.action === 'log' ? 'preview' : 'enforced'
}

function isManagedException(rule: CloudflareWafRule): boolean {
  return rule.definition?.kind === 'managed_exception'
}

function RuleEditor({ editor, onClose, submit, pending }: { editor: Editor; onClose: () => void; submit: (values: RuleFormValues) => void; pending: boolean }) {
  const t = useT()
  const [form] = Form.useForm<RuleFormValues>()
  const ruleset = editor?.ruleset
  const definition = editor?.rule?.definition
  const phase = ruleset?.phase
  const isException = editor?.mode === 'exception'
  const isEdit = editor?.mode === 'edit'
  const title = isException ? t('cloud.waf.exceptionTitle') : isEdit ? t('cloud.waf.editRule') : t('cloud.waf.createRule')
  const utf8Limit = (limit: number) => async (_: unknown, value: string | undefined) => {
    if (value && new TextEncoder().encode(value).length > limit) throw new Error(t('cloud.waf.byteLimit', { n: limit }))
  }
  return (
    <Modal title={title} open={editor !== null} onCancel={onClose} destroyOnHidden okText={isException ? t('cloud.waf.confirmWeakening') : isEdit ? t('btn.save') : t('btn.create')} cancelText={t('btn.cancel')} confirmLoading={pending} onOk={async () => { try { submit(await form.validateFields()) } catch { /* Form renders field validation errors. */ } }}>
      <Form form={form} layout="vertical" preserve={false} initialValues={definitionValues(isException ? undefined : definition)}>
        {isException && <Alert type="warning" showIcon message={t('cloud.waf.exceptionWarning')} style={{ marginBottom: 16 }} />}
        <Form.Item name="reference" label={t('cloud.waf.reference')} rules={[{ required: true }, { validator: utf8Limit(90) }]}><Input autoComplete="off" /></Form.Item>
        <Form.Item name="description" label={t('cloud.waf.description')} rules={[{ required: true }, { validator: utf8Limit(500) }]}><Input autoComplete="off" /></Form.Item>
        <Form.Item name="expression" label={t('cloud.waf.expression')} rules={[{ required: true }, { validator: utf8Limit(4096) }]}><Input.TextArea rows={4} autoComplete="off" /></Form.Item>
        {phase === 'managed' && !isException && <Form.Item name="managedRulesetId" label={t('cloud.waf.managedRulesetId')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item>}
        {isException && <Form.Item name="managedRulesetIdsText" label={t('cloud.waf.managedRulesetIds')} rules={[{ required: true }]}><Input autoComplete="off" /></Form.Item>}
        {isException && <Form.Item name="securityConfirmation" label={t('cloud.waf.securityConfirmation')} rules={[{ required: true }, { validator: async (_: unknown, value: string | undefined) => { if (value !== CONFIRMATION) throw new Error(t('cloud.waf.confirmationMismatch')) } }]}><Input autoComplete="off" /></Form.Item>}
        {phase === 'custom' || phase === 'rate_limit' ? <Form.Item name="action" label={t('cloud.waf.action')} rules={[{ required: true }]}><Select options={ACTIONS.map((value) => ({ value, label: t(`cloud.waf.action.${value}`) }))} /></Form.Item> : null}
        {phase === 'rate_limit' && <>
          <Form.Item name="characteristics" label={t('cloud.waf.characteristics')} rules={[{ required: true }]}><Select mode="multiple" options={['ip_source', 'colo'].map((value) => ({ value, label: t(`cloud.waf.characteristic.${value}`) }))} /></Form.Item>
          <Form.Item name="requestsPerPeriod" label={t('cloud.waf.requests')} rules={[{ required: true }]}><InputNumber min={1} max={1_000_000} style={{ width: '100%' }} /></Form.Item>
          <Form.Item name="periodSecs" label={t('cloud.waf.period')} rules={[{ required: true }]}><Select options={[10, 60, 120, 300, 600, 3600].map((value) => ({ value, label: `${value}` }))} /></Form.Item>
          <Form.Item name="mitigationTimeoutSecs" label={t('cloud.waf.timeout')} rules={[{ required: true }]}><Select options={[10, 30, 60, 120, 300, 600, 1800, 3600].map((value) => ({ value, label: `${value}` }))} /></Form.Item>
        </>}
        {!isEdit && <Form.Item name="position" label={t('cloud.waf.position')}><InputNumber min={1} style={{ width: '100%' }} placeholder={t('cloud.waf.positionAppend')} /></Form.Item>}
      </Form>
    </Modal>
  )
}

function SecurityConfirm({ target, onClose, submit, pending }: { target: SecurityTarget; onClose: () => void; submit: () => void; pending: boolean }) {
  const t = useT()
  const [value, setValue] = useState('')
  return <Modal title={t(target?.mode === 'delete' ? 'cloud.waf.deleteTitle' : 'cloud.waf.downgradeTitle')} open={target !== null} onCancel={onClose} destroyOnHidden okText={t('cloud.waf.confirmWeakening')} okButtonProps={{ danger: true, disabled: value !== CONFIRMATION }} confirmLoading={pending} onOk={submit} cancelText={t('btn.cancel')}>
    <Alert type="warning" showIcon message={t(target?.mode === 'delete' ? 'cloud.waf.deleteWarning' : 'cloud.waf.downgradeWarning')} />
    <Typography.Paragraph style={{ marginTop: 16 }}>{t('cloud.waf.confirmPrompt', { value: CONFIRMATION })}</Typography.Paragraph>
    <Input value={value} onChange={(event) => setValue(event.target.value)} autoComplete="off" />
  </Modal>
}

export default function CloudflareWafPage() {
  const t = useT()
  const client = useQueryClient()
  const canAccounts = useCan('provider-accounts:read')
  const canDns = useCan('cloudflare-dns:read')
  const canRead = useCan('cloudflare-waf:read')
  const canWrite = useCan('cloudflare-waf:write')
  const canOrder = useCan('cloudflare-waf:order')
  const canException = useCan('cloudflare-waf:exception')
  const canWeaken = useCan('cloudflare-waf:security-weaken')
  const canAccess = canAccounts && canDns && canRead
  const [accountId, setAccountId] = useState<string>()
  const [zone, setZone] = useState<CloudflareZone>()
  const [editor, setEditor] = useState<Editor>(null)
  const [security, setSecurity] = useState<SecurityTarget>(null)
  const [result, setResult] = useState<MutationResult>(null)
  const accountsQuery = useQuery({ queryKey: ['cloud-provider-accounts'], queryFn: cloudApi.listAccounts, enabled: canAccounts })
  const accounts = useMemo(() => (accountsQuery.data?.data ?? []).filter((account) => account.provider === 'cloudflare'), [accountsQuery.data?.data])
  const selected = selectedCloudflareAccount(accounts, accountId)
  const zones = useInfiniteQuery({ queryKey: ['cloudflare-dns', accountId, 'zones'], queryFn: ({ pageParam }) => cloudflareDnsApi.listZones(accountId!, pageParam), initialPageParam: undefined as string | undefined, getNextPageParam: (last) => last.data?.nextCursor, enabled: canAccess && accountId !== undefined })
  const inventory = useQuery({ queryKey: [...WAF_KEY, accountId, zone?.zoneId], queryFn: () => cloudflareWafApi.listRulesets(accountId!, zone!.zoneId), enabled: canAccess && accountId !== undefined && zone !== undefined })
  const rulesets = inventory.data?.data?.rulesets ?? []
  const phase = (value: CloudflareWafPhase) => rulesets.find((ruleset) => ruleset.phase === value)
  const invalidate = () => client.invalidateQueries({ queryKey: WAF_KEY })
  const finish = () => { setEditor(null); setSecurity(null); setResult('applied'); message.success(t('cloud.waf.applied')); invalidate() }
  const mutation = useMutation({ mutationFn: async ({ type, ruleset, rule, values, order }: { type: 'create' | 'edit' | 'exception' | 'order' | 'downgrade' | 'delete'; ruleset: CloudflareWafRuleset; rule?: CloudflareWafRule; values?: WafRuleValues; order?: CloudflareWafPosition }) => {
    if (type === 'create') return cloudflareWafApi.create(accountId!, zone!.zoneId, ruleset, values!, order)
    if (type === 'edit') return cloudflareWafApi.update(accountId!, zone!.zoneId, ruleset, rule!, values!)
    if (type === 'exception') return cloudflareWafApi.setManagedException(accountId!, zone!.zoneId, ruleset, values!, order!)
    if (type === 'order') return cloudflareWafApi.order(accountId!, zone!.zoneId, ruleset, rule!, order!)
    if (type === 'downgrade') return cloudflareWafApi.securityWeaken(accountId!, zone!.zoneId, ruleset, rule!, values!)
    return cloudflareWafApi.delete(accountId!, zone!.zoneId, ruleset, rule!)
  }, onSuccess: finish, onError: (error) => { setResult(errorResult(error)); setEditor(null); setSecurity(null) } })
  const openEditor = (ruleset: CloudflareWafRuleset, rule?: CloudflareWafRule, mode: 'create' | 'edit' | 'exception' = rule ? 'edit' : 'create') => { setResult(null); setEditor({ ruleset, rule, mode }) }
  const submitEditor = (values: RuleFormValues) => {
    if (!editor) return
    const clean = standardValues(values)
    const rulePosition = position(values)
    mutation.mutate({ type: editor.mode, ruleset: editor.ruleset, rule: editor.rule, values: clean, order: editor.mode === 'exception' ? rulePosition ?? { type: 'first' } : rulePosition })
  }
  const openOrder = (ruleset: CloudflareWafRuleset, rule: CloudflareWafRule) => {
    let targetPosition = rule.position + 1
    Modal.confirm({ title: t('cloud.waf.orderTitle'), content: <InputNumber data-testid="cloudflare-waf-order" min={1} defaultValue={targetPosition} style={{ width: '100%' }} onChange={(value) => { if (value) targetPosition = Number(value) }} />, okText: t('btn.save'), cancelText: t('btn.cancel'), onOk: () => mutation.mutateAsync({ type: 'order', ruleset, rule, order: { type: 'index', index: targetPosition } }) })
  }
  const zonesList = zones.data?.pages.flatMap((page) => page.data?.items ?? []) ?? []
  const alert = result === null ? null : <Alert style={{ marginBottom: 16 }} showIcon type={result === 'applied' ? 'success' : result === 'ambiguous' ? 'warning' : 'error'} message={t(`cloud.waf.result.${result}`)} />
  return <div>
    <PageHeader title={t('cloud.waf.title')} subtitle={t('cloud.waf.subtitle')} actions={<Button icon={<ReloadOutlined />} onClick={() => { setResult(null); invalidate(); zones.refetch() }}>{t('btn.refresh')}</Button>} />
    {!canRead && <Alert type="warning" showIcon message={t('cloud.permission.wafDenied')} />}
    {canRead && !canAccounts && <Alert type="warning" showIcon message={t('cloud.permission.wafAccountDenied')} />}
    {canRead && canAccounts && !canDns && <Alert type="warning" showIcon message={t('cloud.permission.wafZoneDenied')} />}
    {canAccess && <Space direction="vertical" size={16} style={{ width: '100%' }}>
      {alert}
      <Space wrap><Typography.Text>{t('cloud.waf.account')}</Typography.Text><Select data-testid="cloudflare-waf-account" style={{ minWidth: 260 }} value={accountId} onChange={(value) => { setAccountId(value); setZone(undefined); setResult(null) }} options={accounts.map((account) => ({ value: account.accountId, label: `${account.displayName} (${account.accountId})` }))} placeholder={t('cloud.waf.selectAccount')} /></Space>
      {!selected && accountId === undefined && <Alert type="info" showIcon message={t('cloud.waf.accountHint')} />}
      {selected && <Table size="small" rowKey="zoneId" loading={zones.isLoading} dataSource={zonesList} title={() => t('cloud.waf.zones')} columns={[
        { title: t('cloud.col.name'), dataIndex: 'name' },
        { title: t('cloud.col.status'), dataIndex: 'status', render: (value: string) => <Tag>{value}</Tag> },
        { title: t('col.actions'), render: (_, item: CloudflareZone) => <Button data-testid="cloudflare-waf-zone-open" size="small" onClick={() => { setZone(item); setResult(null) }}>{t('cloud.waf.open')}</Button> },
      ]} />}
      {zone && <><Descriptions size="small" bordered column={2} items={[{ key: 'zone', label: t('cloud.waf.zone'), children: zone.name }, { key: 'id', label: t('cloud.waf.zoneId'), children: zone.zoneId }]} />
        <Tabs items={PHASES.map((value) => {
          const ruleset = phase(value)
          if (!ruleset) return { key: value, label: t(`cloud.waf.phase.${value}`), children: <Empty description={t('cloud.waf.noInventory')} /> }
          const blockers = ruleset.rules.filter((rule) => rule.ownership !== 'center_owned' || !rule.definition)
          return { key: value, label: t(`cloud.waf.phase.${value}`), children: <Space direction="vertical" size={12} style={{ width: '100%' }}>
            <Space wrap><Tag color={availabilityColor(ruleset.availability)}>{t(`cloud.waf.availability.${ruleset.availability}`)}</Tag>{ruleset.rulesetId && <Tag>{t('cloud.waf.version', { value: ruleset.version ?? '—' })}</Tag>}{canWrite && (ruleset.availability === 'available' || ruleset.availability === 'entry_point_absent') && <Button data-testid={`cloudflare-waf-create-${value}`} icon={<PlusOutlined />} onClick={() => openEditor(ruleset)}>{t('cloud.waf.createRule')}</Button>}{value === 'managed' && canException && ruleset.availability === 'available' && <Button data-testid="cloudflare-waf-exception" onClick={() => openEditor(ruleset, undefined, 'exception')}>{t('cloud.waf.addException')}</Button>}</Space>
            {ruleset.availability !== 'available' && ruleset.availability !== 'entry_point_absent' && <Alert type="warning" showIcon message={t('cloud.waf.availabilityHint')} />}
            {ruleset.availability === 'entry_point_absent' && <Alert type="info" showIcon message={t('cloud.waf.entryPointHint')} />}
            {blockers.length > 0 && <Alert type="info" showIcon message={t('cloud.waf.opaqueBlocker', { n: blockers.length })} />}
            <Table size="small" rowKey="ruleId" loading={inventory.isLoading} dataSource={ruleset.rules} pagination={false} columns={[
              { title: t('cloud.waf.position'), dataIndex: 'position', render: (item: number) => item + 1 },
              { title: t('cloud.waf.reference'), render: (_, rule: CloudflareWafRule) => rule.definition?.reference ?? rule.ruleId },
              { title: t('cloud.waf.action'), dataIndex: 'action', render: (item: string) => <Tag>{item}</Tag> },
              { title: t('cloud.waf.enabled'), dataIndex: 'enabled', render: (item: boolean) => <Tag color={item ? 'green' : 'red'}>{t(`cloud.waf.enabled.${item}`)}</Tag> },
              { title: t('cloud.waf.effectiveState'), render: (_, rule: CloudflareWafRule) => <Tag color={effectiveState(rule) === 'enforced' ? 'green' : effectiveState(rule) === 'preview' ? 'gold' : 'default'}>{t(`cloud.waf.effective.${effectiveState(rule)}`)}</Tag> },
              { title: t('cloud.waf.ownership'), dataIndex: 'ownership', render: (item: string) => <Tag color={item === 'center_owned' ? 'blue' : 'gold'}>{t(`cloud.waf.ownership.${item}`)}</Tag> },
              { title: t('cloud.waf.versionLabel'), dataIndex: 'version' },
              { title: t('col.actions'), render: (_, rule: CloudflareWafRule) => <Space>{rule.definition && rule.ownership === 'center_owned' && !isManagedException(rule) && canWrite && <Button size="small" onClick={() => openEditor(ruleset, rule)}>{t('btn.edit')}</Button>}{rule.definition && rule.ownership === 'center_owned' && canOrder && <Button size="small" onClick={() => openOrder(ruleset, rule)}>{t('cloud.waf.order')}</Button>}{rule.definition && rule.ownership === 'center_owned' && canWeaken && <>{!isManagedException(rule) && <Button size="small" danger onClick={() => { setSecurity({ ruleset, rule, mode: 'downgrade' }); setResult(null) }}>{t('cloud.waf.downgrade')}</Button>}<Button size="small" danger onClick={() => { setSecurity({ ruleset, rule, mode: 'delete' }); setResult(null) }}>{t('btn.delete')}</Button></>}</Space> },
            ]} />
          </Space> }
        })} />
      </>}
    </Space>}
    <RuleEditor editor={editor} onClose={() => setEditor(null)} pending={mutation.isPending} submit={submitEditor} />
    <SecurityConfirm target={security} onClose={() => setSecurity(null)} pending={mutation.isPending} submit={() => { if (!security) return; mutation.mutate({ type: security.mode, ruleset: security.ruleset, rule: security.rule, values: standardValues(definitionValues(security.rule.definition)) }) }} />
  </div>
}
