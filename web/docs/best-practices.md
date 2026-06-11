# Edgion Controller 最佳实践与开发规范

> 版本: v1.0  
> 日期: 2024-12-25  
> 参考项目: Kong, APISIX, Traefik

## 📋 目录

- [1. 业界经验总结](#1-业界经验总结)
- [2. UI/UX 设计规范](#2-uiux-设计规范)
- [3. 代码规范](#3-代码规范)
- [4. 常见问题与解决方案](#4-常见问题与解决方案)
- [5. 性能优化指南](#5-性能优化指南)
- [6. 安全最佳实践](#6-安全最佳实践)

---

## 1. 业界经验总结

### 1.1 Kong/Konga 的经验

#### ✅ 值得借鉴

**资源关系可视化**
```
Gateway ──→ HTTPRoute ──→ Service ──→ EndpointSlice
            ├─→ EdgionPlugins
            └─→ EdgionTls
```
- **实现方式**：使用 D3.js 或 ReactFlow 绘制依赖图
- **应用场景**：帮助用户理解资源关系，排查配置问题
- **优先级**：Phase 2（可选）

**配置快照（Snapshot）**
- **功能**：保存当前所有配置的快照，支持一键回滚
- **实现方式**：
  ```typescript
  interface Snapshot {
    id: string;
    timestamp: number;
    resources: {
      [kind: string]: any[];
    };
    description: string;
  }
  ```
- **优先级**：Phase 2

**批量导入/导出**
- **功能**：支持导出所有配置为 YAML 文件，支持批量导入
- **实现**：
  ```typescript
  // 导出
  const exportConfig = () => {
    const allResources = await fetchAllResources();
    const yaml = stringify(allResources);
    downloadFile('edgion-config.yaml', yaml);
  };
  
  // 导入
  const importConfig = async (file: File) => {
    const content = await file.text();
    const resources = parse(content);
    await batchCreateResources(resources);
  };
  ```

#### ❌ 需要避免的坑

**问题 1：大量资源加载缓慢**
- **症状**：一次性加载 1000+ 资源，前端卡顿
- **解决**：
  - 后端实现分页（每页 20-50 条）
  - 前端使用虚拟滚动（react-window）
  - 懒加载详情信息

**问题 2：资源关系复杂难理解**
- **症状**：用户不知道删除 Service 会影响哪些 Route
- **解决**：
  - 删除前显示依赖检查（Phase 2）
  - 提供依赖图谱可视化

---

### 1.2 APISIX Dashboard 的经验

#### ✅ 值得借鉴

**插件配置表单化**
- **问题**：让用户手写插件 JSON 配置太复杂
- **解决**：基于 JSON Schema 自动生成表单
  ```typescript
  // 插件 Schema
  const pluginSchema = {
    type: 'object',
    properties: {
      rate_limit: {
        type: 'object',
        properties: {
          count: { type: 'number', title: '请求数' },
          time_window: { type: 'number', title: '时间窗口(秒)' }
        }
      }
    }
  };
  
  // 使用 react-jsonschema-form 自动生成表单
  <Form schema={pluginSchema} onSubmit={handleSubmit} />
  ```

**Monaco Editor 最佳实践**
```typescript
import Editor from '@monaco-editor/react';

const YamlEditor = ({ value, onChange }) => {
  const handleEditorDidMount = (editor, monaco) => {
    // 配置 YAML 语法
    monaco.languages.yaml.yamlDefaults.setDiagnosticsOptions({
      validate: true,
      schemas: [
        {
          uri: 'http://edgion.io/httproute.schema.json',
          fileMatch: ['*.yaml'],
          schema: httpRouteSchema
        }
      ]
    });
  };
  
  return (
    <Editor
      height="600px"
      language="yaml"
      value={value}
      onChange={onChange}
      onMount={handleEditorDidMount}
      options={{
        minimap: { enabled: false },
        fontSize: 14,
        lineNumbers: 'on',
        scrollBeyondLastLine: false,
        automaticLayout: true,
        tabSize: 2,
      }}
    />
  );
};
```

**实时日志流**
- **实现**：使用 WebSocket 或 SSE 推送日志
- **UI**：自动滚动到最新、支持搜索过滤
- **优先级**：Phase 2（可选）

#### ❌ 需要避免的坑

**问题 1：表单验证滞后**
- **症状**：用户提交后才发现验证错误，体验差
- **解决**：
  ```typescript
  // 使用 react-hook-form 实时验证
  const { register, formState: { errors } } = useForm({
    mode: 'onChange', // 实时验证
    resolver: yupResolver(schema)
  });
  ```

**问题 2：YAML 编辑器无提示**
- **症状**：用户不知道有哪些字段可配置
- **解决**：
  - 集成 JSON Schema 提供自动补全
  - 显示字段说明（hover 提示）
  - 提供配置模板

---

### 1.3 Traefik Dashboard 的经验

#### ✅ 值得借鉴

**极简设计哲学**
- **原则**：只展示必要信息，避免信息过载
- **应用**：
  - Dashboard 首页只显示核心指标
  - 详情页按需展开
  - 减少嵌套层级

**嵌入式部署**
- **优点**：单一二进制文件，部署简单
- **实现**：使用 `rust-embed` 将前端嵌入后端
- **注意**：开发时前后端分离，生产时嵌入

**实时流量可视化**
- **功能**：显示当前请求流向
- **实现**：SVG 动画 + WebSocket 数据流
- **优先级**：Phase 3（可选）

#### ❌ 需要避免的坑

**问题：功能过于简单**
- **症状**：Traefik Dashboard 只能查看，不能编辑
- **教训**：Edgion 需要完整的 CRUD 功能

---

## 2. UI/UX 设计规范

### 2.1 布局规范

#### 左侧导航栏设计

```
宽度: 240px (固定)
背景: #001529 (Ant Design 默认深色)

层级结构:
├─ 📊 Dashboard (首页)
├─ 🛣️ 路由管理 (展开)
│  ├─ HTTPRoute
│  ├─ GRPCRoute
│  ├─ TCPRoute
│  ├─ UDPRoute
│  └─ TLSRoute
├─ 🔧 服务管理
│  ├─ Service
│  └─ EndpointSlice
├─ 🔒 安全配置
│  ├─ TLS
│  └─ Secret
└─ 🔌 插件管理
   ├─ Plugins
   └─ Metadata
```

**交互规则**：
- 当前选中项高亮显示
- 支持折叠/展开子菜单
- 移动端自动收起为汉堡菜单

#### 内容区布局

```
┌─ 顶部操作栏 ──────────────────────────────┐
│ [面包屑导航]          [刷新] [用户] [语言] │
├───────────────────────────────────────────┤
│                                            │
│          主内容区                          │
│          (白色背景)                        │
│                                            │
└───────────────────────────────────────────┘

边距: 24px
卡片间距: 16px
```

### 2.2 组件设计规范

#### 资源状态标识

```typescript
const StatusBadge = ({ status }: { status: string }) => {
  const config = {
    'Active': { color: 'success', icon: '✅', text: '正常' },
    'Error': { color: 'error', icon: '❌', text: '错误' },
    'Pending': { color: 'warning', icon: '⏳', text: '等待中' },
  };
  
  const { color, icon, text } = config[status] || config['Pending'];
  
  return (
    <Badge status={color} text={`${icon} ${text}`} />
  );
};
```

#### 操作按钮规范

| 操作 | 图标 | 颜色 | 位置 |
|-----|------|------|------|
| 查看 | 👁️ Eye | 默认 | Actions 列第1位 |
| 编辑 | ✏️ Edit | Primary | Actions 列第2位 |
| 删除 | 🗑️ Delete | Danger | Actions 列第3位 |
| 创建 | ➕ Plus | Primary | 页面右上角 |
| 刷新 | 🔄 Reload | 默认 | 搜索框右侧 |

**删除确认弹窗**：
```typescript
const handleDelete = (name: string) => {
  Modal.confirm({
    title: '确认删除',
    content: `确定要删除 ${name} 吗？此操作不可恢复。`,
    okText: '确认删除',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      await deleteResource(name);
      message.success('删除成功');
    }
  });
};
```

#### 表格设计规范

**必需列**：
- ☑️ 勾选框（批量操作）
- Name（名称，可点击查看详情）
- Namespace（命名空间，有则显示）
- Status（状态标识）
- Actions（操作按钮）

**可选列**：
- Created At（创建时间）
- Updated At（更新时间）
- Labels（标签，Tag 形式）

**分页配置**：
```typescript
const pagination = {
  defaultPageSize: 20,
  pageSizeOptions: [10, 20, 50, 100],
  showSizeChanger: true,
  showQuickJumper: true,
  showTotal: (total) => `共 ${total} 条记录`,
};
```

### 2.3 交互规范

#### 加载状态

```typescript
// 列表加载
<Spin spinning={isLoading}>
  <Table dataSource={data} />
</Spin>

// 按钮加载
<Button loading={isSaving}>保存</Button>

// 骨架屏（首次加载）
{isInitialLoading && <Skeleton active />}
```

#### 错误提示

```typescript
// 全局错误（Axios 拦截器）
axios.interceptors.response.use(
  response => response,
  error => {
    const msg = error.response?.data?.error || '请求失败';
    message.error(msg);
    return Promise.reject(error);
  }
);

// 表单验证错误
<Form.Item
  name="name"
  rules={[{ required: true, message: '请输入名称' }]}
>
  <Input />
</Form.Item>
```

#### 成功反馈

```typescript
// 操作成功
message.success('创建成功', 2);

// 带跳转
message.success('创建成功', 1, () => {
  navigate('/routes/httproute');
});
```

---

## 3. 代码规范

### 3.1 TypeScript 规范

#### 类型定义

```typescript
// API 响应类型
interface ApiResponse<T> {
  success: boolean;
  data?: T;
  error?: string;
}

interface ListResponse<T> {
  success: boolean;
  data?: T[];
  count: number;
  error?: string;
}

// 资源类型（基于 K8s）
interface K8sResource {
  apiVersion: string;
  kind: string;
  metadata: {
    name: string;
    namespace?: string;
    labels?: Record<string, string>;
    annotations?: Record<string, string>;
  };
  spec: any;
  status?: any;
}

interface HTTPRoute extends K8sResource {
  kind: 'HTTPRoute';
  spec: {
    parentRefs: ParentRef[];
    hostnames?: string[];
    rules: HTTPRouteRule[];
  };
}
```

#### 自定义 Hook 规范

```typescript
// useResource.ts - 通用资源 CRUD Hook
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';

export const useResource = <T extends K8sResource>(
  kind: string,
  namespace?: string
) => {
  const queryClient = useQueryClient();
  const queryKey = namespace ? [kind, namespace] : [kind];
  
  // 查询列表
  const { data, isLoading, error } = useQuery({
    queryKey,
    queryFn: () => api.listResources<T>(kind, namespace),
    staleTime: 5 * 60 * 1000, // 5分钟
  });
  
  // 创建
  const createMutation = useMutation({
    mutationFn: (resource: T) => api.createResource(kind, resource),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
      message.success('创建成功');
    },
  });
  
  // 删除
  const deleteMutation = useMutation({
    mutationFn: (name: string) => api.deleteResource(kind, namespace, name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
      message.success('删除成功');
    },
  });
  
  return {
    resources: data?.data || [],
    isLoading,
    error,
    create: createMutation.mutateAsync,
    delete: deleteMutation.mutateAsync,
  };
};

// 使用示例
const HTTPRouteList = () => {
  const { resources, isLoading, create, delete: del } = useResource<HTTPRoute>('httproute');
  
  return (
    <Table
      loading={isLoading}
      dataSource={resources}
      columns={[...]}
    />
  );
};
```

### 3.2 组件规范

#### 文件组织

```
components/
├── ResourceTable/
│   ├── index.tsx           # 导出
│   ├── ResourceTable.tsx   # 主组件
│   ├── columns.tsx         # 表格列定义
│   ├── types.ts            # 类型定义
│   └── styles.module.css   # 样式（可选）
```

#### 组件模板

```typescript
// ResourceTable.tsx
import { Table, Button, Space } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import type { K8sResource } from '@/types';

interface ResourceTableProps<T extends K8sResource> {
  resources: T[];
  loading?: boolean;
  onView?: (resource: T) => void;
  onEdit?: (resource: T) => void;
  onDelete?: (resource: T) => void;
  onBatchDelete?: (resources: T[]) => void;
}

export const ResourceTable = <T extends K8sResource>({
  resources,
  loading,
  onView,
  onEdit,
  onDelete,
  onBatchDelete,
}: ResourceTableProps<T>) => {
  const [selectedRowKeys, setSelectedRowKeys] = useState<string[]>([]);
  
  const columns: ColumnsType<T> = [
    {
      title: '名称',
      dataIndex: ['metadata', 'name'],
      key: 'name',
      sorter: (a, b) => a.metadata.name.localeCompare(b.metadata.name),
    },
    {
      title: '命名空间',
      dataIndex: ['metadata', 'namespace'],
      key: 'namespace',
    },
    {
      title: '操作',
      key: 'actions',
      render: (_, record) => (
        <Space>
          <Button type="link" onClick={() => onView?.(record)}>查看</Button>
          <Button type="link" onClick={() => onEdit?.(record)}>编辑</Button>
          <Button type="link" danger onClick={() => onDelete?.(record)}>删除</Button>
        </Space>
      ),
    },
  ];
  
  return (
    <>
      {selectedRowKeys.length > 0 && (
        <Alert
          message={`已选 ${selectedRowKeys.length} 项`}
          action={
            <Button danger onClick={() => onBatchDelete?.(selectedResources)}>
              批量删除
            </Button>
          }
        />
      )}
      <Table
        rowSelection={{
          selectedRowKeys,
          onChange: setSelectedRowKeys,
        }}
        columns={columns}
        dataSource={resources}
        loading={loading}
        rowKey={(record) => `${record.metadata.namespace}/${record.metadata.name}`}
      />
    </>
  );
};
```

### 3.3 API 封装规范

```typescript
// api/client.ts
import axios from 'axios';

export const apiClient = axios.create({
  baseURL: import.meta.env.VITE_API_BASE_URL || '/api/v1',
  timeout: 30000,
  headers: {
    'Content-Type': 'application/json',
  },
});

// 请求拦截器
apiClient.interceptors.request.use(
  (config) => {
    // 添加认证 token（Phase 2）
    // const token = getToken();
    // if (token) {
    //   config.headers.Authorization = `Bearer ${token}`;
    // }
    return config;
  },
  (error) => Promise.reject(error)
);

// 响应拦截器
apiClient.interceptors.response.use(
  (response) => response,
  (error) => {
    const msg = error.response?.data?.error || '请求失败';
    message.error(msg);
    return Promise.reject(error);
  }
);

// api/resources.ts
import { apiClient } from './client';
import type { ApiResponse, ListResponse, K8sResource } from './types';

export const resourceApi = {
  // 列出资源
  async list<T extends K8sResource>(
    kind: string,
    namespace?: string
  ): Promise<ListResponse<T>> {
    const url = namespace
      ? `/namespaced/${kind}/${namespace}`
      : `/namespaced/${kind}`;
    const { data } = await apiClient.get(url);
    return data;
  },
  
  // 获取单个资源
  async get<T extends K8sResource>(
    kind: string,
    namespace: string,
    name: string
  ): Promise<T> {
    const { data } = await apiClient.get(`/namespaced/${kind}/${namespace}/${name}`);
    return data;
  },
  
  // 创建资源
  async create<T extends K8sResource>(
    kind: string,
    namespace: string,
    resource: T
  ): Promise<ApiResponse<string>> {
    const { data } = await apiClient.post(
      `/namespaced/${kind}/${namespace}`,
      resource,
      { headers: { 'Content-Type': 'application/yaml' } }
    );
    return data;
  },
  
  // 更新资源
  async update<T extends K8sResource>(
    kind: string,
    namespace: string,
    name: string,
    resource: T
  ): Promise<ApiResponse<string>> {
    const { data } = await apiClient.put(
      `/namespaced/${kind}/${namespace}/${name}`,
      resource,
      { headers: { 'Content-Type': 'application/yaml' } }
    );
    return data;
  },
  
  // 删除资源
  async delete(
    kind: string,
    namespace: string,
    name: string
  ): Promise<ApiResponse<string>> {
    const { data } = await apiClient.delete(`/namespaced/${kind}/${namespace}/${name}`);
    return data;
  },
};
```

---

## 4. 常见问题与解决方案

### 4.1 YAML 编辑相关

**问题 1：YAML 格式错误**

```typescript
// 使用 js-yaml 验证
import { load } from 'js-yaml';

const validateYaml = (content: string): { valid: boolean; error?: string } => {
  try {
    load(content);
    return { valid: true };
  } catch (e) {
    return { valid: false, error: e.message };
  }
};
```

**问题 2：缩进问题**

```typescript
// Monaco Editor 配置
options={{
  tabSize: 2,
  insertSpaces: true,
  detectIndentation: false,
}}
```

### 4.2 性能问题

**问题：大量资源渲染卡顿**

```typescript
// 使用虚拟滚动（react-window）
import { FixedSizeList } from 'react-window';

const VirtualList = ({ items }) => (
  <FixedSizeList
    height={600}
    itemCount={items.length}
    itemSize={50}
    width="100%"
  >
    {({ index, style }) => (
      <div style={style}>{items[index].name}</div>
    )}
  </FixedSizeList>
);
```

### 4.3 中英双语切换

```typescript
// i18n/index.ts
import { ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import enUS from 'antd/locale/en_US';

const App = () => {
  const [locale, setLocale] = useState('zh-CN');
  
  const antdLocale = locale === 'zh-CN' ? zhCN : enUS;
  
  return (
    <ConfigProvider locale={antdLocale}>
      <YourApp />
    </ConfigProvider>
  );
};
```

---

## 5. 性能优化指南

### 5.1 React 优化

```typescript
// 使用 memo 避免不必要的重渲染
export const ResourceCard = memo(({ resource }: { resource: K8sResource }) => {
  return <Card>{resource.metadata.name}</Card>;
});

// 使用 useMemo 缓存计算结果
const filteredResources = useMemo(() => {
  return resources.filter(r => r.metadata.name.includes(searchText));
}, [resources, searchText]);

// 使用 useCallback 缓存函数
const handleDelete = useCallback((name: string) => {
  deleteResource(name);
}, [deleteResource]);
```

### 5.2 网络优化

```typescript
// React Query 配置
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5 * 60 * 1000,     // 5分钟内不重新请求
      cacheTime: 10 * 60 * 1000,    // 缓存保留10分钟
      refetchOnWindowFocus: false,   // 窗口聚焦不自动刷新
      retry: 1,                      // 失败重试1次
    },
  },
});
```

### 5.3 代码分割

```typescript
// 路由懒加载
const Dashboard = lazy(() => import('@/pages/Dashboard'));
const HTTPRouteList = lazy(() => import('@/pages/Routes/HTTPRouteList'));

<Routes>
  <Route path="/" element={<Suspense fallback={<Spin />}><Dashboard /></Suspense>} />
  <Route path="/routes/http" element={<Suspense fallback={<Spin />}><HTTPRouteList /></Suspense>} />
</Routes>
```

---

## 6. 安全最佳实践

### 6.1 输入验证

```typescript
// XSS 防护：Ant Design 已自动转义
<Input value={userInput} />  // 安全

// YAML 内容验证
const sanitizeYaml = (content: string): string => {
  // 移除危险字符
  return content.replace(/<script>/gi, '');
};
```

### 6.2 CORS 配置

```rust
// 后端 Rust 代码
use tower_http::cors::{CorsLayer, Any};

let cors = CorsLayer::new()
    .allow_origin(["http://localhost:5173".parse().unwrap()])
    .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
    .allow_headers(Any);
```

### 6.3 错误信息脱敏

```typescript
// 不要暴露内部路径
❌ "Failed to read /var/lib/edgion/config/route.yaml"
✅ "Failed to load resource configuration"
```

---

## 7. 测试建议

### 7.1 单元测试

```typescript
// ResourceTable.test.tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { ResourceTable } from './ResourceTable';

describe('ResourceTable', () => {
  it('should render resources', () => {
    const resources = [
      { metadata: { name: 'test-route', namespace: 'edge' } }
    ];
    
    render(<ResourceTable resources={resources} />);
    
    expect(screen.getByText('test-route')).toBeInTheDocument();
  });
  
  it('should call onDelete when delete button clicked', () => {
    const onDelete = jest.fn();
    render(<ResourceTable resources={[...]} onDelete={onDelete} />);
    
    fireEvent.click(screen.getByText('删除'));
    
    expect(onDelete).toHaveBeenCalled();
  });
});
```

### 7.2 E2E 测试（Playwright）

```typescript
// e2e/httproute.spec.ts
import { test, expect } from '@playwright/test';

test('create HTTPRoute', async ({ page }) => {
  await page.goto('http://localhost:5173/routes/http');
  
  // 点击创建按钮
  await page.click('text=创建');
  
  // 填写 YAML
  await page.fill('.monaco-editor', `
    apiVersion: gateway.networking.k8s.io/v1
    kind: HTTPRoute
    metadata:
      name: test-route
      namespace: edge
  `);
  
  // 保存
  await page.click('text=保存');
  
  // 验证成功
  await expect(page.locator('text=创建成功')).toBeVisible();
});
```

---

## 8. 故障排查指南

### 8.1 常见错误

| 错误信息 | 原因 | 解决方案 |
|---------|------|---------|
| CORS policy error | 后端未配置 CORS | 检查 tower-http CORS 配置 |
| Network Error | 后端未启动 | 启动 edgion-controller |
| 422 Validation Error | Schema 验证失败 | 检查 YAML 格式和必填字段 |
| 404 Not Found | 资源不存在 | 确认资源名称和 namespace |

### 8.2 调试技巧

```typescript
// 开启 React Query DevTools
import { ReactQueryDevtools } from '@tanstack/react-query-devtools';

<QueryClientProvider client={queryClient}>
  <App />
  <ReactQueryDevtools initialIsOpen={false} />
</QueryClientProvider>

// 开启 Axios 请求日志
axios.interceptors.request.use(config => {
  console.log('[Request]', config.method?.toUpperCase(), config.url);
  return config;
});
```

---

## 附录：Checklist

### 上线前检查清单

- [ ] 所有资源类型支持 CRUD
- [ ] YAML 编辑器功能正常
- [ ] 批量删除功能正常
- [ ] 搜索/过滤功能正常
- [ ] 中英双语切换正常
- [ ] 错误提示友好
- [ ] 加载状态显示
- [ ] 响应式布局（移动端）
- [ ] 生产构建无错误
- [ ] 嵌入式部署正常

---

**本文档持续更新，欢迎贡献！** 🎉

