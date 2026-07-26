import { useEffect, useMemo, useState } from 'react'
import { Alert, Button, Card, Form, Input, Modal, Select, Space, Table, Tag, message } from 'antd'
import { ReloadOutlined, SyncOutlined } from '@ant-design/icons'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'
import {
  globalResourcesApi,
  type DurableGlobalResource,
  type GlobalResourceApplyResult,
  type GlobalResourceKind,
  type GlobalResourcePlan,
  type JsonObject,
} from '@/api/globalResources'

const kinds: GlobalResourceKind[] = ['HTTPRoute', 'GRPCRoute', 'EdgionPlugins', 'EdgionConfigData', 'ReferenceGrant']

export default function GlobalResourceSyncPage() {
  const t = useT()
  const [items, setItems] = useState<DurableGlobalResource[]>([])
  const [selected, setSelected] = useState<DurableGlobalResource | null>(null)
  const [plan, setPlan] = useState<GlobalResourcePlan | null>(null)
  const [result, setResult] = useState<GlobalResourceApplyResult | null>(null)
  const [loading, setLoading] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)
  const [form] = Form.useForm()

  const load = async () => {
    setLoading(true)
    try {
      const page = await globalResourcesApi.syncList({ limit: 100 })
      setItems(page.data)
      if (selected) setSelected(page.data.find((item) => item.id === selected.id) ?? null)
    } catch { message.error('Unable to load GlobalResource desired state') }
    finally { setLoading(false) }
  }
  // Load once on mount; subsequent refreshes are explicit.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { void load() }, [])

  const openCreate = () => {
    form.setFieldsValue({ kind: 'EdgionConfigData', namespace: 'edgion-data', clusters: '', template: '{\n  "apiVersion": "v1",\n  "kind": "EdgionConfigData",\n  "metadata": { "name": "example", "namespace": "edgion-data" },\n  "spec": {}\n}' })
    setEditorOpen(true)
  }
  const save = async (values: { id: string; kind: GlobalResourceKind; namespace: string; clusters: string; template: string }) => {
    let template: JsonObject
    try { template = JSON.parse(values.template) as JsonObject } catch { message.error('Template must be valid JSON'); return }
    const desired = {
      displayName: values.id,
      resourceKind: values.kind,
      targetNamespace: values.namespace,
      templateDocument: template,
      targetSelector: values.clusters.trim() ? { type: 'cluster_ids', clusters: values.clusters.split(',').map((v) => v.trim()).filter(Boolean) } : { type: 'all' },
      syncPolicy: { mode: 'manual', adoption: 'never', prune: 'retain' },
    }
    try {
      const saved = selected
        ? await globalResourcesApi.syncReplace(selected.id, desired, selected.generation)
        : await globalResourcesApi.syncCreate({ id: values.id, desired })
      setSelected(saved); setPlan(null); setResult(null); setEditorOpen(false); await load()
      message.success('GlobalResource saved')
    } catch { message.error('GlobalResource save failed') }
  }
  const makePlan = async () => {
    if (!selected) return
    try { setPlan(await globalResourcesApi.syncPlan(selected.id)); setResult(null) } catch { message.error('Plan failed') }
  }
  const apply = async () => {
    if (!selected || !plan) return
    Modal.confirm({ title: 'Apply GlobalResource?', content: 'This performs a fenced manual sync to the planned clusters. Unknown outcomes are not retried.', okText: 'Apply', okButtonProps: { danger: true }, onOk: async () => {
      try { setResult(await globalResourcesApi.syncApply(selected.id, { planToken: plan.planToken, generation: selected.generation, targetClusters: plan.targetClusters })) } catch { message.error('Apply failed before a result was returned') }
    } })
  }

  const columns = useMemo(() => [
    { title: 'ID', dataIndex: 'id' },
    { title: 'Kind', render: (_: unknown, row: DurableGlobalResource) => row.desired.resourceKind },
    { title: 'Namespace', render: (_: unknown, row: DurableGlobalResource) => row.desired.targetNamespace },
    { title: 'Generation', dataIndex: 'generation' },
    { title: 'Revision', render: (_: unknown, row: DurableGlobalResource) => <Tag>{row.desiredRevision.slice(0, 18)}…</Tag> },
  ], [])

  return <>
    <PageHeader title={t('center.nav.globalResourceSync')} subtitle="Durable desired state, fresh plan, and manual selective sync" actions={<Space><Button icon={<ReloadOutlined />} onClick={() => void load()} loading={loading}>Refresh</Button><Button type="primary" onClick={openCreate}>Create</Button></Space>} />
    <Space direction="vertical" style={{ width: '100%' }} size="large">
      <Card title="GlobalResource desired state"><Table rowKey="id" loading={loading} columns={columns} dataSource={items} onRow={(row) => ({ onClick: () => { setSelected(row); setPlan(null); setResult(null) } })} pagination={false} /></Card>
      {selected && <Card title={`${selected.id} · generation ${selected.generation}`} extra={<Space><Button onClick={() => { form.setFieldsValue({ id: selected.id, kind: selected.desired.resourceKind, namespace: selected.desired.targetNamespace, clusters: selected.desired.targetSelector.clusters?.join(', ') ?? '', template: JSON.stringify(selected.desired.templateDocument, null, 2) }); setEditorOpen(true) }}>Edit</Button><Button icon={<SyncOutlined />} onClick={() => void makePlan()}>Plan</Button></Space>}>
        {plan && <><Alert type={plan.applicable ? 'info' : 'warning'} message={plan.applicable ? `Plan ready for ${plan.targetClusters.length} cluster(s)` : 'Plan is not applicable'} description={plan.targets.map((target) => `${target.cluster}: ${target.state} (${target.reason ?? 'n/a'})`).join(' · ')} /><Button danger disabled={!plan.applicable} onClick={() => void apply()} style={{ marginTop: 16 }}>Apply exact plan</Button></>}
        {result && <Table rowKey="cluster" style={{ marginTop: 16 }} pagination={false} dataSource={result.targets} columns={[{ title: 'Cluster', dataIndex: 'cluster' }, { title: 'Outcome', dataIndex: 'outcome', render: (value: string) => <Tag color={value === 'created' || value === 'updated' ? 'green' : 'orange'}>{value}</Tag> }, { title: 'Status', dataIndex: 'statusCode' }]} />}
      </Card>}
    </Space>
    <Modal title={selected ? 'Edit GlobalResource' : 'Create GlobalResource'} open={editorOpen} onCancel={() => setEditorOpen(false)} onOk={() => form.submit()} okText="Save">
      <Form form={form} layout="vertical" onFinish={(values) => void save(values)}><Form.Item name="id" label="ID" rules={[{ required: true }]}><Input disabled={Boolean(selected)} /></Form.Item><Form.Item name="kind" label="Kind" rules={[{ required: true }]}><Select options={kinds.map((kind) => ({ label: kind, value: kind }))} /></Form.Item><Form.Item name="namespace" label="Platform namespace" rules={[{ required: true }]}><Input /></Form.Item><Form.Item name="clusters" label="Target clusters (comma-separated; empty = all)"><Input /></Form.Item><Form.Item name="template" label="Template JSON" rules={[{ required: true }]}><Input.TextArea autoSize={{ minRows: 10, maxRows: 24 }} /></Form.Item></Form>
    </Modal>
  </>
}
