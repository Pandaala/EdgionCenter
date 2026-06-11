---
name: types-and-utils-pattern
description: TypeScript 类型定义和 YAML 工具函数的标准模式
---

# 类型定义 & 工具函数模式

## 类型定义模式

文件位置：`src/types/{resource}/index.ts`

参考：
- `src/types/gateway-api/httproute.ts`（305 行）
- `src/types/edgion-plugins/index.ts`（156 行）

### 标准结构

```typescript
// 1. 枚举/联合类型
export type PathMatchType = 'Exact' | 'PathPrefix' | 'RegularExpression'

// 2. 子类型（从叶到根定义）
export interface SomeMatch {
  type?: MatchType
  value: string
}

// 3. 规则/Spec 类型
export interface ResourceSpec {
  // 字段匹配后端 YAML Schema
}

// 4. 主资源类型
export interface ResourceType {
  apiVersion: string   // e.g., 'gateway.networking.k8s.io/v1'
  kind: string         // e.g., 'HTTPRoute'
  metadata: K8sMetadata
  spec: ResourceSpec
  status?: any
}
```

### 要点
- 类型名与 K8s 资源名一致（PascalCase）
- 可选字段用 `?` 标注
- 复用 `K8sMetadata` 和 `K8sResource`（from `src/api/types.ts`）
- apiVersion 和 kind 用字面量类型或 string

## 工具函数模式

文件位置：`src/utils/{resource}.ts`

参考：
- `src/utils/httproute.ts`（190 行）
- `src/utils/edgionplugins.ts`（129 行）

### 必须实现的函数

```typescript
import * as yaml from 'js-yaml'
import type { ResourceType } from '@/types/{resource}'

/**
 * 创建空资源对象（用于 create 模式）
 */
export function createEmptyResource(): ResourceType {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'ResourceName',
    metadata: {
      name: '',
      namespace: 'default',
    },
    spec: {
      // 所有必填字段的默认值
    },
  }
}

/**
 * 规范化后端返回的数据（补充缺失字段、统一格式）
 */
export function normalizeResource(raw: any): ResourceType {
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1',
    kind: raw.kind || 'ResourceName',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
    },
    spec: {
      // 递归规范化 spec 字段
    },
  }
}

/**
 * 对象 → YAML 字符串
 */
export function resourceToYaml(resource: ResourceType): string {
  // 清理空字段后序列化
  const clean = removeEmpty(resource)
  return yaml.dump(clean, { lineWidth: -1, noRefs: true })
}

/**
 * YAML 字符串 → 对象
 */
export function yamlToResource(yamlStr: string): ResourceType {
  const raw = yaml.load(yamlStr) as any
  return normalizeResource(raw)
}
```

### 辅助函数

- `removeEmpty(obj)` — 递归删除 null、undefined、空数组、空对象
- 计数/统计函数（按需，如 EdgionPlugins 的 `countPluginsByStage`）

### 要点

1. **createEmpty** 返回完整结构，所有必填字段有默认值
2. **normalize** 处理后端返回数据的各种边界情况（字段缺失、null 值）
3. **toYaml** 先清理空字段，避免输出 `field: null` 或 `field: []`
4. **fromYaml** 用 `yaml.load` + normalize 双重保障
5. `lineWidth: -1` 避免 YAML 长行换行
6. `noRefs: true` 避免 YAML 锚点引用
