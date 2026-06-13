---
name: editor-modal-pattern
description: Resource editor Modal pattern — Form/YAML dual tabs, bidirectional sync, Zod validation
---

# Editor Modal Pattern

Reference implementations:
- `src/components/ResourceEditor/HTTPRoute/HTTPRouteEditor.tsx` (230 lines)
- `src/components/ResourceEditor/EdgionPlugins/EdgionPluginsEditor.tsx` (208 lines)

## Standard Structure

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

  // Initialize data
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

  // Form → YAML sync (when switching to YAML tab)
  const handleTabChange = (key: string) => {
    if (key === 'yaml') {
      setYamlContent(resourceToYaml(formData))
    } else if (key === 'form') {
      try {
        const parsed = yamlToResource(yamlContent)
        setFormData(parsed)
      } catch (e) {
        message.error('YAML parse failed')
        return // do not switch tab
      }
    }
    setActiveTab(key)
  }

  // Create mutation
  const createMutation = useMutation({
    mutationFn: (yamlStr: string) =>
      resourceApi.create(RESOURCE_KIND, formData.metadata.namespace || 'default', yamlStr),
    onSuccess: () => {
      message.success('Created successfully')
      queryClient.invalidateQueries({ queryKey: [RESOURCE_KIND] })
      onClose()
    },
  })

  // Update mutation
  const updateMutation = useMutation({
    mutationFn: (yamlStr: string) =>
      resourceApi.update(RESOURCE_KIND, formData.metadata.namespace || 'default',
        formData.metadata.name, yamlStr),
    onSuccess: () => {
      message.success('Updated successfully')
      queryClient.invalidateQueries({ queryKey: [RESOURCE_KIND] })
      onClose()
    },
  })

  // Submit
  const handleSubmit = () => {
    const yamlStr = activeTab === 'yaml' ? yamlContent : resourceToYaml(formData)
    // Optional: Zod validation
    if (mode === 'create') {
      createMutation.mutate(yamlStr)
    } else {
      updateMutation.mutate(yamlStr)
    }
  }

  return (
    <Modal
      title={mode === 'create' ? 'Create Resource' : mode === 'edit' ? 'Edit Resource' : 'View Resource'}
      open={visible}
      onCancel={onClose}
      width={900}
      footer={mode === 'view' ? null : [
        <Button key="cancel" onClick={onClose}>Cancel</Button>,
        <Button key="submit" type="primary" onClick={handleSubmit}
          loading={createMutation.isPending || updateMutation.isPending}>
          {mode === 'create' ? 'Create' : 'Save'}
        </Button>,
      ]}
    >
      <Tabs activeKey={activeTab} onChange={handleTabChange}>
        <Tabs.TabPane tab="Form" key="form">
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

## Key Points

1. **Dual-tab bidirectional sync**: Form→YAML serializes on tab switch; YAML→Form parses on tab switch
2. **Three modes**: create (empty form), edit (pre-filled data), view (read-only, no submit button)
3. **Separate mutations**: create and update use different mutations (different API endpoints)
4. **YAML-first submission**: if the active tab is YAML, submit the YAML content directly
5. **Modal width**: 900px (suitable for side-by-side form layout)
6. **Loading state**: submit button shows `isPending` as loading

## Form Section Pattern

Complex resource forms are split into Section components by feature:

```
ResourceForm.tsx
├── MetadataSection.tsx       # name, namespace, labels, annotations
├── ParentRefsSection.tsx     # Gateway binding (route-type resources)
├── HostnamesSection.tsx      # hostname list
├── RulesSection.tsx          # routing rules
│   ├── MatchesEditor.tsx     # match conditions
│   └── BackendRefsEditor.tsx # backend references
└── ...
```

Each Section receives `data`, `onChange`, and `readOnly` props.
