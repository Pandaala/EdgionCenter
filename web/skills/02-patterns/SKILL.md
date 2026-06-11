---
name: dashboard-patterns
description: Edgion Controller 开发模式——列表页、编辑器、类型定义、工具函数的标准模板
---

# 开发模式

本目录记录了 Edgion Controller 中已验证的开发模式。新增资源页面时严格遵循这些模式，保持一致性。

## 文件清单

| 文件 | 内容 |
|------|------|
| [01-list-page.md](01-list-page.md) | 列表页模式：Table + 搜索 + 批量操作 + React Query |
| [02-editor-modal.md](02-editor-modal.md) | 编辑器模式：Modal + Form/YAML 双标签 + 双向同步 |
| [03-types-and-utils.md](03-types-and-utils.md) | 类型定义 + YAML 工具函数模式 |
| [04-i18n-rules.md](04-i18n-rules.md) | **多语言规范（强制）**：t() 用法、key 速查、新增 key 流程 |

## 新资源页面开发检查清单

1. [ ] `src/types/{resource}/index.ts` — 类型定义
2. [ ] `src/utils/{resource}.ts` — 工具函数（createEmpty, normalize, toYaml, fromYaml）
3. [ ] `src/schemas/{resource}/` — Zod 验证 Schema（可选，复杂表单需要）
4. [ ] `src/components/ResourceEditor/{Resource}/{Resource}Editor.tsx` — 编辑器 Modal
5. [ ] `src/components/ResourceEditor/{Resource}/{Resource}Form.tsx` — 表单
6. [ ] `src/components/ResourceEditor/{Resource}/sections/` — 表单区段（按需）
7. [ ] `src/pages/{Category}/{Resource}List.tsx` — 列表页
8. [ ] `src/App.tsx` — 添加路由
9. [ ] `src/components/Layout/MainLayout.tsx` — 菜单项（如果需要新增）
10. [ ] `src/api/types.ts` — ResourceKind 枚举（如果新增 kind）
11. [ ] **`src/i18n/en.ts` + `src/i18n/zh.ts`** — 新增菜单 key（`nav.*`）及页面所需的新 key

## 多语言（i18n）强制规范

> **所有用户可见文字必须通过 `t()` 输出，禁止硬编码中文或双语混合字符串。**

详细规范见 `skills/02-patterns/04-i18n-rules.md`（项目内）或 `ws2/skills/02-dashboard/04-i18n.md`（工作区级）。

**快速用法**：
```typescript
import { useT } from '@/i18n'
const t = useT()  // 必须在 React 组件函数体内

// 基础
t('btn.create')        // "Create" / "创建"
t('col.name')          // "Name" / "名称"
t('msg.deleteOk')      // "Deleted successfully" / "删除成功"

// 带参数
t('modal.create', { resource: 'Gateway' })   // "Create Gateway"
t('msg.batchDeleteOk', { n: 5 })             // "5 resources deleted"
t('table.totalItems', { n: total })          // "Total: 42"
t('confirm.deleteMsg', { name })             // "Are you sure...?"
t('msg.createFailed', { err: e.message })    // "Create failed: ..."

// 技术名词不翻译（直接字面量）
`${t('btn.create')} Gateway`     // ✅
`${t('btn.create')} HTTPRoute`   // ✅
```

**新增 key 流程**：
1. 先查 `ws2/skills/02-dashboard/04-i18n.md` 确认无可复用 key
2. **同时**在 `en.ts` 和 `zh.ts` 添加，两文件必须保持 key 一致
3. 在组件中使用 `t('new.key')`
