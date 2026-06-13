---
name: dashboard-patterns
description: EdgionCenter dashboard development patterns — standard templates for list pages, editors, type definitions, and utility functions
---

# Development Patterns

This directory documents proven development patterns used in the EdgionCenter dashboard. When adding new resource pages, follow these patterns strictly to maintain consistency.

## File Index

| File | Content |
|------|---------|
| [01-list-page.md](01-list-page.md) | List page pattern: Table + search + bulk operations + React Query |
| [02-editor-modal.md](02-editor-modal.md) | Editor pattern: Modal + Form/YAML dual tabs + bidirectional sync |
| [03-types-and-utils.md](03-types-and-utils.md) | Type definitions + YAML utility function patterns |
| [04-i18n-rules.md](04-i18n-rules.md) | **i18n rules (mandatory)**: t() usage, key quick reference, adding new keys |

## New Resource Page Checklist

1. [ ] `src/types/{resource}/index.ts` — type definitions
2. [ ] `src/utils/{resource}.ts` — utility functions (createEmpty, normalize, toYaml, fromYaml)
3. [ ] `src/schemas/{resource}/` — Zod validation schema (optional, for complex forms)
4. [ ] `src/components/ResourceEditor/{Resource}/{Resource}Editor.tsx` — editor Modal
5. [ ] `src/components/ResourceEditor/{Resource}/{Resource}Form.tsx` — form
6. [ ] `src/components/ResourceEditor/{Resource}/sections/` — form sections (as needed)
7. [ ] `src/pages/{Category}/{Resource}List.tsx` — list page
8. [ ] `src/App.tsx` — add route
9. [ ] `src/components/Layout/MainLayout.tsx` — menu item (if a new menu entry is needed)
10. [ ] `src/api/types.ts` — ResourceKind enum (if adding a new kind)
11. [ ] **`src/i18n/en.ts` + `src/i18n/zh.ts`** — add menu keys (`nav.*`) and any new keys required by the page

## i18n Mandatory Rules

> **All user-visible text must be output via `t()`. Hard-coded strings and bilingual mixed strings are forbidden.**

See `skills/02-patterns/04-i18n-rules.md` for the full i18n specification.

**Quick usage**:
```typescript
import { useT } from '@/i18n'
const t = useT()  // must be called inside a React component function body

// Basic
t('btn.create')        // "Create"
t('col.name')          // "Name"
t('msg.deleteOk')      // "Deleted successfully"

// With parameters
t('modal.create', { resource: 'Gateway' })   // "Create Gateway"
t('msg.batchDeleteOk', { n: 5 })             // "5 resources deleted"
t('table.totalItems', { n: total })          // "Total: 42"
t('confirm.deleteMsg', { name })             // "Are you sure...?"
t('msg.createFailed', { err: e.message })    // "Create failed: ..."

// Technical terms are not translated (use string literals directly)
`${t('btn.create')} Gateway`     // ✅
`${t('btn.create')} HTTPRoute`   // ✅
```

**Adding new keys**:
1. Check `04-i18n-rules.md` to confirm no existing key can be reused
2. **Simultaneously** add to `en.ts` and `zh.ts` — both files must stay in sync
3. Use `t('new.key')` in the component
