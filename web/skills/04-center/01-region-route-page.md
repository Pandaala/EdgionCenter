---
name: center-region-route-page
description: Center 联邦 RegionRoute 页面设计：Global PM 页 + Service PM 页、数据获取策略、Failover 操作、冲突检测。
---

# Center RegionRoute 页面设计

## 相关文件

| 文件 | 说明 |
|------|------|
| `src/pages/Center/ClusterRegionRoutePage.tsx` | Cluster PM 页面 |
| `src/pages/Center/RegionRoutePage.tsx` | Service PM 页面（含 Failover） |
| `src/api/center.ts` | Center API 层 |
| `src/components/Layout/CenterLayout.tsx` | 侧栏导航 |

## 页面结构

侧栏 RegionRoute 下有两个子菜单：

```
RegionRoute
├── Cluster  → /region-routes/cluster   (ClusterRegionRoutePage)
└── Service  → /region-routes/service  (RegionRoutePage)
```

## 后端 API

### Center 聚合端点

| 方法 | 路径 | 返回 |
|------|------|------|
| GET | `center/cluster-region-pms` | `{ namespace, name, controllers[] }[]` |
| GET | `center/service-region-pms` | `{ namespace, name, controllers[] }[]` |
| POST | `center/cluster-region-pms/failover` | `{ namespace, name, regionName, failoverTo }` → `{ modified, failed }` |
| POST | `center/service-region-pms/failover` | 同上 |

### Controller 端点（通过 `proxy/{ctrlId}/api/v1/...`）

| 路径 | 返回 |
|------|------|
| `cluster-region-pms` | 完整拓扑：`{ namespace, name, myRegion, regions[], keyGet[], hashCalc, routeRules[] }` |
| `service-region-pms` | `{ namespace, name, clusterRef: {ns, name}, regions: [{name, failoverTo?}], refPlugins[] }` |

## Cluster PM 页面

**数据源**：`center/cluster-region-pms`（主表）+ proxy 到 controller 的 `cluster-region-pms`（详情）

**列**：Namespace | Name | Regions

**展开行**：从每个 controller proxy 获取 ClusterRegionRoute 详情，展示 region 拓扑表、冲突检测、routeRules、hashCalc。

**RegionsCell**：从第一个 controller 获取 region 预览（lightweight，每行一个 proxy 请求）。

## Service PM 页面

**数据源**：`center/service-region-pms`（主表）+ proxy 到 controller 的 `service-region-pms` + `cluster-region-pms`（详情）

**列**：Service PM Name | Namespace | Controllers | Regions | Failover

**数据获取优化**：
- 主表的 RegionsCell 使用 `FirstControllerCtx`（React Context），从一个 controller 拉一次所有 service PM + cluster PM，共享给所有行。全页面只发 2 个 proxy 请求。
- 展开行按需从所有 controller 拉数据做冲突检测。

```
RegionRoutePage
├── FirstControllerCtx.Provider       ← 单次获取，context 共享
│   ├── Table
│   │   ├── RegionsCell               ← 从 context 查找，无额外请求
│   │   └── RowActions → FailoverPanel  ← 从 context 获取 canonicalRegions + clusterRef
│   └── ExpandedDetail                ← 按需从所有 controller proxy 获取
```

## Failover 操作

使用 Center 的扇出端点，不再逐个 proxy PUT 到每个 controller：

```typescript
centerApi.clusterPmFailover(namespace, name, regionName, failoverTo)
// POST center/cluster-region-pms/failover → 自动扇出到所有 controller
// 返回 { modified: N, failed: N }
```

**流程**：
1. FailoverPanel 展示 canonicalRegions（从 context 获取）
2. 用户修改 failoverTo 下拉框
3. 点击 Apply → 只对变更的 region 发 POST
4. 成功后 invalidate React Query 缓存 → 自动刷新 + 关闭 Popover

## 冲突检测

跨 controller 对比 ClusterRegionRoute 的 regions 字段：

```typescript
interface RegionConflict {
  regionName: string
  field: 'hashRange' | 'backendEndpoint' | 'failoverTo'
  items: Array<{ controllerId: string; value: string }>
}
```

同一 region 在不同 controller 上的 hashRange/endpoint/failoverTo 不一致时产生冲突。
冲突在 ExpandedDetail 展开行中以 Warning Alert 展示。

## 关键类型

```typescript
// center.ts
interface RegionPmSummary { namespace: string; name: string; controllers: string[] }
interface ClusterRegionPmDetail { namespace: string; name: string; myRegion: string; regions: RegionDef[]; keyGet; hashCalc; routeRules; routeByKeyConfMatch }
interface ServiceRegionPmDetail { namespace: string; name: string; clusterRef: { namespace: string; name: string }; regions: Array<{ name: string; failoverTo?: string }>; refPlugins: string[] }
interface RegionDef { name: string; hashRange: [number, number]; backendEndpoint: string; tls: boolean; failoverTo?: string }
```
