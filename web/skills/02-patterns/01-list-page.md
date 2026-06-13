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
