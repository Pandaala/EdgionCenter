---
name: list-page-pattern
description: 列表页开发模式——以 HTTPRouteList 和 EdgionPluginsList 为参考的标准列表页模板
---

# 列表页模式

参考实现：
- `src/pages/Routes/HTTPRouteList.tsx`（210 行）
- `src/pages/Plugins/EdgionPluginsList.tsx`（265 行）

## 标准结构

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

  // 查询
  const { data, isLoading, refetch } = useQuery({
    queryKey: [RESOURCE_KIND],
    queryFn: () => resourceApi.listAll<ResourceType>(RESOURCE_KIND),
  })

  // 删除 Mutation
  const deleteMutation = useMutation({
    mutationFn: ({ namespace, name }: { namespace: string; name: string }) =>
      resourceApi.delete(RESOURCE_KIND, namespace, name),
    onSuccess: () => {
      message.success('删除成功')
      queryClient.invalidateQueries({ queryKey: [RESOURCE_KIND] })
    },
  })

  // 过滤数据
  const filteredData = (data?.data || []).filter(item =>
    item.metadata.name.includes(searchText) ||
    item.metadata.namespace?.includes(searchText)
  )

  // 表格列定义
  const columns = [
    { title: '名称', dataIndex: ['metadata', 'name'], key: 'name' },
    { title: '命名空间', dataIndex: ['metadata', 'namespace'], key: 'namespace' },
    // ... 资源特有列
    {
      title: '操作',
      key: 'actions',
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => openEditor('view', record)}>查看</Button>
          <Button size="small" onClick={() => openEditor('edit', record)}>编辑</Button>
          <Button size="small" danger onClick={() => confirmDelete(record)}>删除</Button>
        </Space>
      ),
    },
  ]

  return (
    <div>
      {/* 工具栏 */}
      <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
        <Space>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => openEditor('create')}>
            创建
          </Button>
          <Button danger icon={<DeleteOutlined />} disabled={!selectedRowKeys.length}
            onClick={batchDelete}>
            批量删除
          </Button>
        </Space>
        <Space>
          <Input prefix={<SearchOutlined />} placeholder="搜索..." value={searchText}
            onChange={e => setSearchText(e.target.value)} allowClear />
          <Button icon={<ReloadOutlined />} onClick={() => refetch()} />
        </Space>
      </div>

      {/* 表格 */}
      <Table
        rowKey={record => `${record.metadata.namespace}/${record.metadata.name}`}
        columns={columns}
        dataSource={filteredData}
        loading={isLoading}
        rowSelection={{ selectedRowKeys, onChange: setSelectedRowKeys }}
        pagination={{ pageSize: 20 }}
      />

      {/* 编辑器 */}
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

## 关键要点

1. **React Query 管理数据获取**：queryKey 用 kind 值，mutation 成功后 invalidateQueries
2. **搜索过滤在前端**：简单 includes 过滤 name 和 namespace
3. **批量删除**：用 rowSelection 收集选中项，确认后并行删除
4. **编辑器状态**：`{ visible, mode, resource? }` 三态管理 create/edit/view
5. **rowKey**：用 `{namespace}/{name}` 组合确保唯一
6. **加载状态**：Table 的 `loading` 属性绑定 `isLoading`
