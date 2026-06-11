---
name: i18n-rules
description: 多语言使用规范——强制规则、完整 key 分类速查、新增 key 流程（在 dashboard 项目内使用）
---

# 多语言使用规范

> **强制规范**：所有用户可见文字必须通过 `t()` 输出。禁止硬编码中文，禁止双语混写。

**完整 key 清单**见工作区级文档：`ws2/skills/02-dashboard/04-i18n.md`

## Hook 用法

```typescript
import { useT } from '@/i18n'
const t = useT()  // 必须在 React 组件函数体内（Hook 规则）
```

## 常用 Key 速查

### 按钮

```typescript
t('btn.create')       // Create / 创建
t('btn.edit')         // Edit / 编辑
t('btn.view')         // View / 查看
t('btn.delete')       // Delete / 删除
t('btn.save')         // Save / 保存
t('btn.cancel')       // Cancel / 取消
t('btn.close')        // Close / 关闭
t('btn.refresh')      // Refresh / 刷新
t('btn.trigger')      // Trigger / 触发
t('btn.batchDelete')  // Batch Delete / 批量删除
```

### 表格列头

```typescript
t('col.name')        // Name / 名称
t('col.namespace')   // Namespace / 命名空间
t('col.actions')     // Actions / 操作
t('col.type')        // Type / 类型
t('col.status')      // Status / 状态
// 更多见 ws2/skills/02-dashboard/04-i18n.md
```

### 弹窗标题（参数化）

```typescript
t('modal.create', { resource: 'Gateway' })  // Create Gateway / 创建 Gateway
t('modal.edit',   { resource: 'Gateway' })  // Edit Gateway / 编辑 Gateway
t('modal.view',   { resource: 'Gateway' })  // View Gateway / 查看 Gateway
```

### 标签页

```typescript
t('tab.form')   // Form / 表单
t('tab.yaml')   // YAML / YAML
```

### 消息提示

```typescript
t('msg.createOk')                       // Created successfully / 创建成功
t('msg.updateOk')                       // Updated successfully / 更新成功
t('msg.deleteOk')                       // Deleted successfully / 删除成功
t('msg.batchDeleteOk', { n: 5 })        // 5 resources deleted / 成功删除 5 个资源
t('msg.createFailed', { err: e.msg })   // Create failed: ... / 创建失败: ...
t('msg.tabSwitchFailed', { err })       // Tab switch failed: ...
t('msg.opFailed', { err })              // Operation failed: ...
```

### 确认对话框（完整模式）

```typescript
Modal.confirm({
  title:      t('confirm.deleteTitle'),                   // Confirm Delete
  content:    t('confirm.deleteMsg', { name }),           // Are you sure...?
  okText:     t('confirm.okText'),                        // Confirm Delete
  okType:     'danger',
  cancelText: t('btn.cancel'),
  onOk:       () => deleteMutation.mutate(...),
})

// 批量删除
content: `${t('confirm.batchDeleteMsg', { n: selected.length })} ${t('confirm.deleteIrreversible')}`
```

### 搜索 + 分页

```typescript
<Search placeholder={t('ph.searchNameNs')} />    // 搜索名称/命名空间
<Search placeholder={t('ph.searchName')} />       // 搜索名称
pagination={{ showTotal: (n) => t('table.totalItems', { n }) }}
```

### 批量删除按钮

```typescript
`${t('btn.batchDelete')}${selectedRowKeys.length > 0 ? ` (${selectedRowKeys.length})` : ''}`
```

### 标准 Create 按钮（技术名词不翻译）

```typescript
`${t('btn.create')} Gateway`     // Create Gateway / 创建 Gateway
`${t('btn.create')} HTTPRoute`   // Create HTTPRoute / 创建 HTTPRoute
`${t('btn.create')} EdgionTls`   // Create EdgionTls / 创建 EdgionTls
```

## 技术名词不翻译

以下词汇在两种语言中相同，直接写字符串字面量：

- 资源类型：`HTTPRoute`、`GRPCRoute`、`TCPRoute`、`TLSRoute`、`Gateway`、`GatewayClass`、`EdgionTls`、`EdgionPlugins`、`LinkSys`、`EdgionAcme` 等
- 协议：`HTTP`、`HTTPS`、`TCP`、`TLS`、`UDP`、`gRPC`
- 匹配类型：`Exact`、`PathPrefix`、`RegularExpression`
- 格式：`YAML`、`JSON`

## 新增 Key 流程

1. 查 `ws2/skills/02-dashboard/04-i18n.md` 确认无可复用 key
2. **同时**在 `src/i18n/en.ts` 和 `src/i18n/zh.ts` 中添加：

```typescript
// en.ts
'my.newKey': 'English text with {param}',

// zh.ts  
'my.newKey': '中文文本带 {param}',
```

3. 组件中使用：`t('my.newKey', { param: 'value' })`

> 只更新一个文件 → 另一种语言显示原始 key 字符串（明显 bug）。必须两个文件同步。

## 常见错误

```typescript
// ❌ 硬编码中文
<Button>创建</Button>
{ title: '名称', ... }
message.success('删除成功')

// ❌ 双语混写
<Button>Create / 创建</Button>

// ❌ 在非组件函数中调用 Hook
const t = useT()  // 模块顶层 → React 报错

// ❌ 参数 key 错误
t('msg.batchDeleteOk', { count: 5 })  // 应是 { n: 5 }

// ✅ 正确
const t = useT()  // 在组件函数体内
<Button>{t('btn.create')} Gateway</Button>
{ title: t('col.name'), ... }
message.success(t('msg.deleteOk'))
t('msg.batchDeleteOk', { n: 5 })
```
