import { useState } from 'react'
import { Alert, Input, Space, Table, Tabs, Tag } from 'antd'
import { EditOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import { useQuery } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import type { ResourceKey, ResourceKind } from '@/api/types'
import { resourceApi } from '@/api/resources'
import { useControllerAccess } from '@/hooks/useControllerAccess'
import PermissionAwareButton from '@/components/resource/PermissionAwareButton'
import { resourceActionTestId } from '@/components/resource/testIds'
import PageHeader from '@/components/PageHeader'
import SecretEditor from '@/components/ResourceEditor/Secret/SecretEditor'
import ConfigMapEditor from '@/components/ResourceEditor/ConfigMap/ConfigMapEditor'

type RestrictedKind = Extract<ResourceKind, 'secret' | 'configmap'>

export default function RestrictedDependenciesPage() {
  const { controllerId } = useParams<{ controllerId?: string }>()
  const scope = controllerId ?? null
  const access = useControllerAccess(scope, true)
  const [kind, setKind] = useState<RestrictedKind>('secret')
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState<ResourceKey | null>(null)
  const [editorOpen, setEditorOpen] = useState(false)
  const canList = access.canResource(kind, 'list-keys')
  const query = useQuery({ queryKey: ['restricted-keys', kind, scope], queryFn: () => resourceApi.listKeys(kind), enabled: canList, retry: false })
  const rows = (query.data?.data ?? []).filter((row) => `${row.metadata.namespace}/${row.metadata.name}`.toLowerCase().includes(search.toLowerCase()))
  const title = kind === 'secret' ? 'Secret' : 'ConfigMap'
  const open = (resource: ResourceKey | null) => { setSelected(resource); setEditorOpen(true) }
  return <div>
    <PageHeader title="Restricted dependencies" subtitle="Metadata-only Secret and ConfigMap management. Values are write-only." actions={<Space>
      <PermissionAwareButton data-testid={resourceActionTestId(kind, 'refresh')} icon={<ReloadOutlined />} resourceKind={kind} resourceVerb="list-keys" onClick={() => query.refetch()}>Refresh</PermissionAwareButton>
      <PermissionAwareButton data-testid={resourceActionTestId(kind, 'create')} type="primary" icon={<PlusOutlined />} resourceKind={kind} resourceVerb="create" onClick={() => open(null)}>Create {title}</PermissionAwareButton>
    </Space>} />
    <Alert type="info" showIcon message="Protected value handling" description="This page only requests resource keys. Replacements start empty and require the complete new content." style={{ marginBottom: 16 }} />
    <Tabs activeKey={kind} onChange={(key) => { setKind(key as RestrictedKind); setSelected(null) }} items={[{ key: 'secret', label: <span data-testid="secret-tab">Secrets</span> }, { key: 'configmap', label: <span data-testid="configmap-tab">ConfigMaps</span> }]} />
    {!canList && <Alert type="warning" showIcon message={access.authorizationPending ? 'Authorization is loading' : `Metadata access denied for ${title}`} description="The request stays disabled until list-keys permission is confirmed." style={{ marginBottom: 16 }} />}
    <Input.Search data-testid={resourceActionTestId(kind, 'search')} allowClear value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search namespace/name" style={{ width: 300, marginBottom: 16 }} />
    <Table<ResourceKey> rowKey={(row) => `${row.metadata.namespace}/${row.metadata.name}`} loading={canList && query.isLoading} dataSource={rows} columns={[
      { title: 'Name', dataIndex: ['metadata', 'name'] },
      { title: 'Namespace', dataIndex: ['metadata', 'namespace'], render: (value) => <Tag>{value || 'default'}</Tag> },
      { title: 'Actions', width: 160, render: (_, row) => <PermissionAwareButton data-testid={resourceActionTestId(kind, 'row-replace')} size="small" icon={<EditOutlined />} resourceKind={kind} resourceVerb="update" onClick={() => open(row)}>Replace</PermissionAwareButton> },
    ]} />
    {kind === 'secret' ? <SecretEditor visible={editorOpen} mode={selected ? 'edit' : 'create'} resource={selected} onClose={() => setEditorOpen(false)} /> : <ConfigMapEditor visible={editorOpen} mode={selected ? 'edit' : 'create'} resource={selected} onClose={() => setEditorOpen(false)} />}
  </div>
}
