# Edgion Controller

Edgion Controller 的 Web 管理界面，基于 React + TypeScript + Ant Design 构建。

## 🚀 快速开始

### 安装依赖

```bash
npm install
```

### 开发环境

启动开发服务器（带热更新）：

```bash
npm run dev
```

访问：http://localhost:5173

**注意**：确保 Edgion Controller 已启动在 `localhost:12101`，Vite 会自动代理 API 请求。

### 生产构建

```bash
npm run build
```

构建产物在 `dist/` 目录，将被嵌入到 Edgion Controller 二进制文件中。

## 📁 项目结构

```
edgion-dashboard/
├── src/
│   ├── api/              # API 客户端
│   │   ├── client.ts     # Axios 配置
│   │   ├── resources.ts  # 资源 CRUD API
│   │   └── types.ts      # 类型定义
│   ├── components/       # 可复用组件
│   │   └── Layout/       # 布局组件
│   ├── pages/            # 页面组件
│   │   ├── Dashboard/    # 首页
│   │   └── Routes/       # 路由管理
│   ├── App.tsx           # 应用入口
│   └── main.tsx          # React 入口
├── docs/                 # 文档
│   ├── architecture-design.md
│   └── best-practices.md
├── vite.config.ts        # Vite 配置（含代理）
└── package.json
```

## 🔧 技术栈

- **React 18** - UI 框架
- **TypeScript** - 类型安全
- **Ant Design** - UI 组件库
- **React Router** - 路由管理
- **React Query** - 服务端状态管理
- **Axios** - HTTP 客户端
- **Vite** - 构建工具

## 📚 文档

详细文档请参阅：
- [架构设计文档](./docs/architecture-design.md)
- [最佳实践与开发规范](./docs/best-practices.md)

## 🛠️ 开发

### 添加新页面

1. 在 `src/pages/` 创建页面组件
2. 在 `src/App.tsx` 添加路由
3. 在 `src/components/Layout/MainLayout.tsx` 添加菜单项

### API 调用

```typescript
import { resourceApi } from '@/api/resources'

// 查询所有 HTTPRoute
const { data } = await resourceApi.listAll('httproute')

// 创建资源
await resourceApi.create('httproute', 'default', yamlContent)
```

## ⚠️ 注意事项

1. **开发环境**：通过 Vite 代理访问后端，无需配置 CORS
2. **生产环境**：前端嵌入到 Controller，同源访问
3. **端口配置**：
   - 前端开发：5173
   - 后端 API：12101

## License

This project is licensed under the [Apache License, Version 2.0](./LICENSE).
