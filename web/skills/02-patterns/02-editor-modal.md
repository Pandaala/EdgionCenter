---
name: editor-modal-pattern
description: 资源编辑器 Modal 模式——Form/YAML 双标签、双向同步、Zod 验证
---

# 编辑器 Modal 模式

参考实现：
- `src/components/ResourceEditor/HTTPRoute/HTTPRouteEditor.tsx`（230 行）
- `src/components/ResourceEditor/EdgionPlugins/EdgionPluginsEditor.tsx`（208 行）

## 标准结构

```typescript
interface ResourceEditorProps {
  visible: boolean
  mode: 'create' | 'edit' | 'view'
  resource?: ResourceType
  onClose: () => void
}

const ResourceEditor: React.FC<ResourceEditorProps> = ({ visible, mode, resource, onClose }) => {
  const [activeTab, setActiveTab] = useState<'form' | 'yaml'>('form')
  const [formData, setFormData] = useState<ResourceType>(createEmptyResource())
  const [yamlContent, setYamlContent] = useState('')
  const queryClient = useQueryClient()

  // 初始化数据
  useEffect(() => {
    if (visible) {
      if (mode === 'create') {
        const empty = createEmptyResource()
        setFormData(empty)
        setYamlContent(resourceToYaml(empty))
      } else if (resource) {
        const normalized = normalizeResource(resource)
        setFormData(normalized)
        setYamlContent(resourceToYaml(normalized))
      }
    }
  }, [visible, mode, resource])

  // Form → YAML 同步（切换到 YAML 标签时）
  const handleTabChange = (key: string) => {
    if (key === 'yaml') {
      setYamlContent(resourceToYaml(formData))
    } else if (key === 'form') {
      try {
        const parsed = yamlToResource(yamlContent)
        setFormData(parsed)
      } catch (e) {
        message.error('YAML 解析失败')
        return // 不切换标签
      }
    }
    setActiveTab(key)
  }

  // 创建 Mutation
  const createMutation = useMutation({
    mutationFn: (yamlStr: string) =>
      resourceApi.create(RESOURCE_KIND, formData.metadata.namespace || 'default', yamlStr),
    onSuccess: () => {
      message.success('创建成功')
      queryClient.invalidateQueries({ queryKey: [RESOURCE_KIND] })
      onClose()
    },
  })

  // 更新 Mutation
  const updateMutation = useMutation({
    mutationFn: (yamlStr: string) =>
      resourceApi.update(RESOURCE_KIND, formData.metadata.namespace || 'default',
        formData.metadata.name, yamlStr),
    onSuccess: () => {
      message.success('更新成功')
      queryClient.invalidateQueries({ queryKey: [RESOURCE_KIND] })
      onClose()
    },
  })

  // 提交
  const handleSubmit = () => {
    const yamlStr = activeTab === 'yaml' ? yamlContent : resourceToYaml(formData)
    // 可选：Zod 验证
    if (mode === 'create') {
      createMutation.mutate(yamlStr)
    } else {
      updateMutation.mutate(yamlStr)
    }
  }

  return (
    <Modal
      title={mode === 'create' ? '创建资源' : mode === 'edit' ? '编辑资源' : '查看资源'}
      open={visible}
      onCancel={onClose}
      width={900}
      footer={mode === 'view' ? null : [
        <Button key="cancel" onClick={onClose}>取消</Button>,
        <Button key="submit" type="primary" onClick={handleSubmit}
          loading={createMutation.isPending || updateMutation.isPending}>
          {mode === 'create' ? '创建' : '保存'}
        </Button>,
      ]}
    >
      <Tabs activeKey={activeTab} onChange={handleTabChange}>
        <Tabs.TabPane tab="表单" key="form">
          <ResourceForm data={formData} onChange={setFormData} readOnly={mode === 'view'} />
        </Tabs.TabPane>
        <Tabs.TabPane tab="YAML" key="yaml">
          <YamlEditor value={yamlContent} onChange={setYamlContent} readOnly={mode === 'view'} />
        </Tabs.TabPane>
      </Tabs>
    </Modal>
  )
}
```

## 关键要点

1. **双标签切换同步**：Form→YAML 在切换时序列化，YAML→Form 在切换时解析
2. **三种模式**：create（空表单）、edit（填充数据）、view（只读，无提交按钮）
3. **Mutation 分离**：create 和 update 用不同 mutation（不同 API 端点）
4. **YAML 优先提交**：如果当前在 YAML 标签，直接提交 YAML 内容
5. **Modal 宽度**：900px（适合左右布局表单）
6. **加载状态**：提交按钮显示 `isPending` loading

## 表单区段模式

复杂资源的表单按功能分拆为 Section 组件：

```
ResourceForm.tsx
├── MetadataSection.tsx       # name, namespace, labels, annotations
├── ParentRefsSection.tsx     # Gateway 绑定（路由类资源）
├── HostnamesSection.tsx      # 主机名列表
├── RulesSection.tsx          # 路由规则
│   ├── MatchesEditor.tsx     # 匹配条件
│   └── BackendRefsEditor.tsx # 后端引用
└── ...
```

每个 Section 接收 `data`, `onChange`, `readOnly` props。
