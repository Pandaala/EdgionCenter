---
name: dashboard-testing
description: Edgion Controller 测试指南——启动后端、加载数据、开发验证流程
---

# 测试指南

## 启动后端测试环境

前端开发需要后端 API 返回真实数据。通过 Edgion 的集成测试基础设施启动。

### 方式一：一键启动（推荐）

```bash
# 在 edgion 项目目录下
cd /Users/caohao/ws2/edgion

# 启动 Controller + Gateway 并加载全部测试数据
./examples/test/scripts/utils/start_all_with_conf.sh

# 或者启动后手动加载数据
./examples/test/scripts/utils/start_all_with_conf.sh --no-load
./examples/test/scripts/utils/load_conf.sh all          # 加载全部
./examples/test/scripts/utils/load_conf.sh http          # 只加载 HTTPRoute
```

### 方式二：手动启动

```bash
cd /Users/caohao/ws2/edgion

# 1. 构建
cargo build

# 2. 启动 Controller
./target/debug/edgion-controller -c config/edgion-controller.toml &

# 3. 启动 Gateway
./target/debug/edgion-gateway -c config/edgion-gateway.toml &

# 4. 用 edgion-ctl 加载配置
./target/debug/edgion-ctl apply examples/test/conf/base/
./target/debug/edgion-ctl apply examples/test/conf/HTTPRoute/
```

## 测试端口

| 服务 | 端口 | 用途 |
|------|------|------|
| Controller Admin API | 12101 | 前端 API 后端（Vite 代理目标） |
| Controller gRPC | 12151 | Gateway 配置同步 |
| Gateway HTTP | 10080 | 数据面 HTTP 代理 |
| Gateway HTTPS | 10443 | 数据面 HTTPS 代理 |
| Gateway Admin | 12001 | Gateway 管理 API |
| 前端 Dev Server | 5173 | Vite 开发服务器 |

## 验证流程

1. **启动后端**：`start_all_with_conf.sh`
2. **加载数据**：`load_conf.sh all`
3. **启动前端**：`cd edgion-dashboard && npm run dev`
4. **浏览器验证**：http://localhost:5173
5. **验证 API**：http://localhost:12101/api/v1/namespaced/httproute（直接测试 API）

## 测试数据目录

```
edgion/examples/test/conf/
├── base/                  # GatewayClass, Gateway, EdgionGatewayConfig, TLS secrets
├── HTTPRoute/             # HTTPRoute 测试用例
├── GRPCRoute/             # GRPCRoute 测试用例
├── TCPRoute/              # TCPRoute 测试用例
├── UDPRoute/              # UDPRoute 测试用例
├── TLSRoute/              # TLSRoute 测试用例
├── EdgionPlugins/         # 插件测试用例
├── EdgionTls/             # TLS 配置测试用例
├── Status/                # 状态更新测试
└── LinkSys/               # 外部集成测试
```

## 验证检查清单

新增页面开发完成后：
- [ ] 列表页加载正常，显示测试数据
- [ ] 搜索过滤正常工作
- [ ] 创建新资源（Form 模式）
- [ ] 创建新资源（YAML 模式）
- [ ] 查看资源详情
- [ ] 编辑资源（Form 模式）
- [ ] 编辑资源（YAML 模式）
- [ ] 删除单个资源
- [ ] 批量删除资源
- [ ] 刷新按钮正常
- [ ] 侧边栏导航高亮正确
- [ ] 无 TypeScript 编译错误
- [ ] 无控制台错误
