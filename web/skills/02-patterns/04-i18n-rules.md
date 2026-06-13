---
name: i18n-rules
description: i18n usage rules — mandatory rules, full key category quick reference, and the workflow for adding new keys (for use within the dashboard project)
---

# i18n Usage Rules

> **Mandatory rule**: All user-visible text must be output via `t()`. Hard-coded strings in any language and bilingual mixed strings are both forbidden.

**The authoritative key list** is defined in the i18n source files: `src/i18n/en.ts` and `src/i18n/zh.ts`.

## Hook Usage

```typescript
import { useT } from '@/i18n'
const t = useT()  // must be called inside a React component function body (Hook rules)
```

## Common Key Quick Reference

### Buttons

```typescript
t('btn.create')       // Create
t('btn.edit')         // Edit
t('btn.view')         // View
t('btn.delete')       // Delete
t('btn.save')         // Save
t('btn.cancel')       // Cancel
t('btn.close')        // Close
t('btn.refresh')      // Refresh
t('btn.trigger')      // Trigger
t('btn.batchDelete')  // Batch Delete
```

### Table Column Headers

```typescript
t('col.name')        // Name
t('col.namespace')   // Namespace
t('col.actions')     // Actions
t('col.type')        // Type
t('col.status')      // Status
// see src/i18n/en.ts for the full list
```

### Modal Titles (parameterized)

```typescript
t('modal.create', { resource: 'Gateway' })  // Create Gateway
t('modal.edit',   { resource: 'Gateway' })  // Edit Gateway
t('modal.view',   { resource: 'Gateway' })  // View Gateway
```

### Tabs

```typescript
t('tab.form')   // Form
t('tab.yaml')   // YAML
```

### Message Notifications

```typescript
t('msg.createOk')                       // Created successfully
t('msg.updateOk')                       // Updated successfully
t('msg.deleteOk')                       // Deleted successfully
t('msg.batchDeleteOk', { n: 5 })        // 5 resources deleted
t('msg.createFailed', { err: e.msg })   // Create failed: ...
t('msg.tabSwitchFailed', { err })       // Tab switch failed: ...
t('msg.opFailed', { err })              // Operation failed: ...
```

### Confirm Dialog (full pattern)

```typescript
Modal.confirm({
  title:      t('confirm.deleteTitle'),                   // Confirm Delete
  content:    t('confirm.deleteMsg', { name }),           // Are you sure...?
  okText:     t('confirm.okText'),                        // Confirm Delete
  okType:     'danger',
  cancelText: t('btn.cancel'),
  onOk:       () => deleteMutation.mutate(...),
})

// Bulk delete
content: `${t('confirm.batchDeleteMsg', { n: selected.length })} ${t('confirm.deleteIrreversible')}`
```

### Search + Pagination

```typescript
<Search placeholder={t('ph.searchNameNs')} />    // search by name/namespace
<Search placeholder={t('ph.searchName')} />       // search by name
pagination={{ showTotal: (n) => t('table.totalItems', { n }) }}
```

### Batch Delete Button

```typescript
`${t('btn.batchDelete')}${selectedRowKeys.length > 0 ? ` (${selectedRowKeys.length})` : ''}`
```

### Standard Create Button (technical terms are not translated)

```typescript
`${t('btn.create')} Gateway`     // Create Gateway
`${t('btn.create')} HTTPRoute`   // Create HTTPRoute
`${t('btn.create')} EdgionTls`   // Create EdgionTls
```

## Technical Terms Are Not Translated

The following terms are identical in both languages — write them as string literals:

- Resource types: `HTTPRoute`, `GRPCRoute`, `TCPRoute`, `TLSRoute`, `Gateway`, `GatewayClass`, `EdgionTls`, `EdgionPlugins`, `LinkSys`, `EdgionAcme`, etc.
- Protocols: `HTTP`, `HTTPS`, `TCP`, `TLS`, `UDP`, `gRPC`
- Match types: `Exact`, `PathPrefix`, `RegularExpression`
- Formats: `YAML`, `JSON`

## Adding New Keys

1. Check `src/i18n/en.ts` and `src/i18n/zh.ts` to confirm no existing key can be reused
2. **Simultaneously** add to `src/i18n/en.ts` and `src/i18n/zh.ts`:

```typescript
// en.ts
'my.newKey': 'English text with {param}',

// zh.ts  
'my.newKey': 'Chinese text with {param}',
```

3. Use in a component: `t('my.newKey', { param: 'value' })`

> Updating only one file causes the other language to display the raw key string — an obvious bug. Both files must always be kept in sync.

## Common Mistakes

```typescript
// ❌ hard-coded string (bypasses t())
<Button>Create</Button>
{ title: 'Name', ... }
message.success('Deleted successfully')

// ❌ bilingual mixed string (do not mix display languages in a single literal)

// ❌ calling Hook outside a component function
const t = useT()  // module top level → React error

// ❌ wrong parameter key
t('msg.batchDeleteOk', { count: 5 })  // should be { n: 5 }

// ✅ correct
const t = useT()  // inside component function body
<Button>{t('btn.create')} Gateway</Button>
{ title: t('col.name'), ... }
message.success(t('msg.deleteOk'))
t('msg.batchDeleteOk', { n: 5 })
```
