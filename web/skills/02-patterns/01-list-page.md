---
name: list-page-pattern
description: List page development pattern — standard list page template based on HTTPRouteList and EdgionPluginsList
---

# List Page Pattern

Reference implementations:
- `src/pages/Routes/HTTPRouteList.tsx` (210 lines)
- `src/pages/Plugins/EdgionPluginsList.tsx` (265 lines)

## Standard Structure

```typescript
import { useState } from 'react'
import { Table, Button, Input, Space, Modal, message } from 'antd'
import { PlusOutlined, DeleteOutlined, SearchOutlined, ReloadOutlined } from '@ant-design/icons'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { ResourceType } from '@/types/{resource}'
import ResourceEditor from '@/components/ResourceEditor/{Resource}/{Resource}Editor'

const RESOURCE_KIND = '{kind}' as const  // e.g., 'httproute'

const ResourceList = () => {
  const [searchText, setSearchText] = useState('')
  const [selectedRowKeys, setSelectedRowKeys] = useState<React.Key[]>([])
  const [editorState, setEditorState] = useState<{
    visible: boolean
    mode: 'create' | 'edit' | 'view'
    resource?: ResourceType
  }>({ visible: false, mode: 'create' })
  
  const queryClient = useQueryClient()

  // Data query
  const { data, isLoading, refetch } = useQuery({
    queryKey: [RESOURCE_KIND],
    queryFn: () => resourceApi.listAll<ResourceType>(RESOURCE_KIND),
  })

  // Delete mutation
  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name }: { namespace: string; name: string }) =>
      resourceApi.delete(RESOURCE_KIND, namespace, name),
    onSuccess: () => {
      message.success('Deleted successfully')
      queryClient.invalidateQueries({ queryKey: [RESOURCE_KIND] })
    },
  })

  // Filter data
  const filteredData = (data?.data || []).filter(item =>
    item.metadata.name.includes(searchText) ||
    item.metadata.namespace?.includes(searchText)
  )

  // Table column definitions
  const columns = [
    { title: 'Name', dataIndex: ['metadata', 'name'], key: 'name' },
    { title: 'Namespace', dataIndex: ['metadata', 'namespace'], key: 'namespace' },
    // ... resource-specific columns
    {
      title: 'Actions',
      key: 'actions',
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => openEditor('view', record)}>View</Button>
          <Button size="small" onClick={() => openEditor('edit', record)}>Edit</Button>
          <Button size="small" danger onClick={() => confirmDelete(record)}>Delete</Button>
        </Space>
      ),
    },
  ]

  return (
    <div>
      {/* Toolbar */}
      <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
        <Space>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>
            Create
          </Button>
          <Button danger icon={<DeleteOutlined />} disabled={!selectedRowKeys.length}
            onClick={batchDelete}>
            Batch Delete
          </Button>
        </Space>
        <Space>
          <Input prefix={<SearchOutlined />} placeholder="Search..." value={searchText}
            onChange={e => setSearchText(e.target.value)} allowClear />
          <Button icon={<ReloadOutlined />} onClick={() => refetch()} />
        </Space>
      </div>

      {/* Table */}
      <Table
        rowKey={record => `${record.metadata.namespace}/${record.metadata.name}`}
        columns={columns}
        dataSource={filteredData}
        loading={isLoading}
        rowSelection={{ selectedRowKeys, onChange: setSelectedRowKeys }}
        pagination={{ pageSize: 20 }}
      />

      {/* Editor */}
      <ResourceEditor
        visible={editorState.visible}
        mode={editorState.mode}
        resource={editorState.resource}
        onClose={() => setEditorState({ ...editorState, visible: false })}
      />
    </div>
  )
}
```

## Key Points

1. **React Query for data fetching**: use the resource kind as the queryKey; call invalidateQueries after a successful mutation
2. **Client-side search filtering**: simple `includes` filter on name and namespace
3. **Bulk delete**: collect selected items via rowSelection, then delete in parallel after confirmation
4. **Editor state**: `{ visible, mode, resource? }` — three-state management for create/edit/view
5. **rowKey**: use `{namespace}/{name}` combination to ensure uniqueness
6. **Loading state**: bind `isLoading` to the Table's `loading` prop


## Source lists and runtime status

Use `useResourceList` for paginated source rows; its cache and HTTP requests bind
explicitly to the Controller captured by the page. Custom queries must do the
same. Do not rely on a mutable global proxy interceptor to choose the target
of a previously constructed request.

Existing status columns use `ResourceStatus` with the kind and entire source row.
Source status takes precedence, preserving Kubernetes multi-writer conditions.
If absent, `useRuntimeResourceStatus` reads the individual processed resource
through Center and accepts only matching Controller, kind, namespace, name and
resourceVersion. Keep processed spec out of source rows, editors and mutations.
Do not add synthetic status to ReferenceGrant or restricted dependency lists.

Observations are limited to mounted rows and four concurrent reads, poll every
15 seconds while active, and refresh after successful source-list refresh even
when resourceVersion is unchanged. Custom list queries must call
`useInvalidateRuntimeStatus(kind, dataUpdatedAt)`. Unmounted queued reads are
cancelled. Missing versions, mismatched observations and read errors must remain
visible without hiding readable source rows. Never render a cached healthy
observation after a failed read, or expose raw remote error details.

For resource-specific status fields, pass a `renderStatus` callback to
`ResourceStatus` (see `AcmeLifecycle`). This retains the same precedence,
identity/version validation and read-failure behavior as Conditions. Callbacks
receive only the observation status; never merge it into the editable resource.


## Expired pagination cursors

`useResourceList` handles HTTP 410 / StalePagination by resetting the exact active
query and fetching page one. Do not replace this with removeQueries: removing the
cache entry does not restart the mounted infinite-query observer. Reset all old
pages/tokens together; never append a fresh page to the old snapshot.

Recovery respects the enabled gate and current Controller identity. Repeated
failures within five seconds do not trigger another automatic reset. A failed
first-page recovery stays visible even after that window; the normal Retry action
remains available. Recovery notices describe an in-progress refresh, not success.
Regression evidence lives in `src/hooks/useResourceList.pagination.test.tsx`.


## Batch selection and Controller changes

Resolve selected row keys against the complete loaded source list, not the search
result. Filtering changes visibility, not the operator's selected set. Capture
namespace/name/resourceVersion in the confirmation payload, and use mutation
variables for the successful count: current selection can change while requests
are running. Keep existing partial-failure handling so only failures stay selected.

ControllerProxy keys its page shell by normalized Controller identity. Navigation
to another Controller discards list selections and open editor drafts, including
when the new Controller has resources with identical namespace/name. Request
clients must still capture their target; remounting is not a transport boundary.
