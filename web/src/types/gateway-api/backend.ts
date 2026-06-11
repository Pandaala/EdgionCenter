/**
 * Gateway API 后端引用相关类型定义
 */

// ============================================
// ParentReference - 父引用
// ============================================

/**
 * ParentReference - 父引用（用于指定 Gateway）
 * 用于多个资源类型（HTTPRoute, GRPCRoute 等）
 */
export interface ParentReference {
  /** API Group，默认 "gateway.networking.k8s.io" */
  group?: string;
  
  /** 资源类型，默认 "Gateway" */
  kind?: string;
  
  /** 命名空间，默认同当前资源的 namespace */
  namespace?: string;
  
  /** Gateway 名称（必填） */
  name: string;
  
  /** Listener 名称（可选，指定 Gateway 的特定 Listener） */
  sectionName?: string;
  
  /** 端口号（可选，1-65535） */
  port?: number;
}

// ============================================
// BackendRef - 后端引用
// ============================================

/**
 * BackendRef - 后端服务引用（完整版，包含 weight）
 * 用于 HTTPRoute, GRPCRoute 等的 backendRefs 字段
 */
export interface BackendRef {
  /** API Group，默认 ""（核心 API） */
  group?: string;
  
  /** 资源类型，默认 "Service" */
  kind?: string;
  
  /** 命名空间，默认同当前资源的 namespace */
  namespace?: string;
  
  /** Service 名称（必填） */
  name: string;
  
  /** 端口号（可选，1-65535） */
  port?: number;
  
  /** 权重（可选，0-1000000，默认 1，用于流量分配） */
  weight?: number;
}

/**
 * BackendObjectReference - 后端对象引用（简化版，无 weight）
 * 用于 RequestMirror 等场景
 */
export interface BackendObjectReference {
  /** API Group */
  group?: string;
  
  /** 资源类型 */
  kind?: string;
  
  /** 命名空间 */
  namespace?: string;
  
  /** 资源名称（必填） */
  name: string;
  
  /** 端口号 */
  port?: number;
}

// ============================================
// LocalObjectReference - 本地对象引用
// ============================================

/**
 * LocalObjectReference - 本地对象引用
 * 用于 ExtensionRef 等场景
 */
export interface LocalObjectReference {
  /** API Group（必填） */
  group: string;
  
  /** 资源类型（必填） */
  kind: string;
  
  /** 资源名称（必填） */
  name: string;
}

// ============================================
// HTTP Header
// ============================================

/**
 * HTTPHeader - HTTP 请求头/响应头
 */
export interface HTTPHeader {
  /** Header 名称（必填） */
  name: string;
  
  /** Header 值（必填） */
  value: string;
}

