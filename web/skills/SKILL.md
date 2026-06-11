---
name: edgion-dashboard-skills
description: Root navigation for the Edgion Controller knowledge base. Read this first, then drill into the relevant subtree.
---

# Edgion Controller Skills

> React 18 + TypeScript + Ant Design 5 前端管理界面，对接 Edgion Controller REST API。
> 管理 Gateway API 标准资源和 Edgion 自定义资源（路由、插件、TLS、服务等）。

## 导航规则

1. **渐进式披露**：本文件 → 分类 SKILL.md → 具体文件。只加载当前任务需要的最小子树。
2. **三层定位**：
   - **理解架构**（前端整体设计） → `01-architecture/`
   - **组件模式**（如何开发页面和编辑器） → `02-patterns/`
   - **资源指南**（每种资源的 Schema 和开发要点） → `03-resources/`
3. **新资源页面**开发需要同时参考 `02-patterns/`（代码模式）和 `03-resources/`（数据 Schema）。

## 快速定位

| 你想了解… | 直接入口 |
|-----------|---------|
| **项目架构** — 目录结构、数据流、技术栈 | [01-architecture/SKILL.md](01-architecture/SKILL.md) |
| **列表页模式** — Table + 搜索 + 批量操作 | [02-patterns/01-list-page.md](02-patterns/01-list-page.md) |
| **编辑器模式** — Modal + Form/YAML 双标签 | [02-patterns/02-editor-modal.md](02-patterns/02-editor-modal.md) |
| **类型 & 工具函数模式** — 类型定义 + YAML 转换 | [02-patterns/03-types-and-utils.md](02-patterns/03-types-and-utils.md) |
| **API 层** — Axios 客户端、resourceApi/clusterResourceApi | [01-architecture/02-api-layer.md](01-architecture/02-api-layer.md) |
| **资源开发指南** — 各资源的 Schema、字段、特殊处理 | [03-resources/SKILL.md](03-resources/SKILL.md) |
| **路由类资源** — HTTPRoute/GRPCRoute/TCPRoute/UDPRoute/TLSRoute | [03-resources/01-routes.md](03-resources/01-routes.md) |
| **基础设施资源** — Gateway/GatewayClass/Service/EndpointSlice | [03-resources/02-infrastructure.md](03-resources/02-infrastructure.md) |
| **安全资源** — EdgionTls/Secret/BackendTLSPolicy | [03-resources/03-security.md](03-resources/03-security.md) |
| **插件资源** — EdgionPlugins/EdgionStreamPlugins/PluginMetaData | [03-resources/04-plugins.md](03-resources/04-plugins.md) |
| **系统配置** — EdgionGatewayConfig/LinkSys/EdgionAcme | [03-resources/05-system.md](03-resources/05-system.md) |
| **测试指南** — 如何启动后端测试环境 | [04-testing/SKILL.md](04-testing/SKILL.md) |
| **Center 联邦管理** — RegionRoute 页面设计、冲突检测、Failover 控制 | [04-center/01-region-route-page.md](04-center/01-region-route-page.md) |

## 目录总览

| # | 目录 | 用途 |
|---|------|------|
| 01 | [architecture/](01-architecture/SKILL.md) | 项目架构：目录结构、数据流、状态管理、API 层、路由 |
| 02 | [patterns/](02-patterns/SKILL.md) | 开发模式：列表页、编辑器 Modal、类型定义、YAML 工具函数 |
| 03 | [resources/](03-resources/SKILL.md) | 资源指南：每种 K8s/Edgion 资源的 Schema、字段映射、开发要点 |
| 04 | [testing/](04-testing/SKILL.md) | 测试：后端启动、测试数据加载、开发验证流程 |
| 05 | [04-center/](04-center/01-region-route-page.md) | Center 联邦：RegionRoute 页面设计、冲突检测、Failover/Sync 控制、Admin 页面 |

## 开发生命周期速查

| Phase | 做什么 | 加载 |
|-------|--------|------|
| 1 了解需求 | 确认资源 Schema、字段定义 | `03-resources/` + Edgion `skills/02-features/03-resources/` |
| 2 理解模式 | 参考已有实现（HTTPRoute/EdgionPlugins） | `02-patterns/` |
| 3 编码实现 | 类型 → 工具函数 → 编辑器 → 列表页 → 路由注册 | `02-patterns/` + `01-architecture/` |
| 4 测试验证 | 启动后端 + 加载数据 + 浏览器验证 | `04-testing/` |

## 已完成的功能

- **全部 20 个 CRUD 页面** — Phase 0-5 已完成，详见 [pages-and-routes](../../skills/02-dashboard/01-pages-and-routes.md)
- **资源拓扑可视化** — `/topology`（React Flow + dagre，用户级 Gateway→Route→Service/Plugin 引用图）
- **Center 模式** — 多 Controller 联邦管理（CenterLayout + ControllerProxy）
