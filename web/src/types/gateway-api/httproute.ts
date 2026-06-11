/**
 * HTTPRoute 专属类型定义
 * 基于 Gateway API v1 规范
 */

import type { K8sResource, K8sObjectMeta, Hostname, PreciseHostname, Duration } from './common';
import type {
  ParentReference,
  BackendRef,
  BackendObjectReference,
  LocalObjectReference,
  HTTPHeader,
} from './backend';

// ============================================
// HTTPRoute 匹配条件类型
// ============================================

/**
 * PathMatchType - 路径匹配类型
 */
export type PathMatchType = 'Exact' | 'PathPrefix' | 'RegularExpression';

/**
 * HeaderMatchType - Header 匹配类型
 */
export type HeaderMatchType = 'Exact' | 'RegularExpression';

/**
 * QueryParamMatchType - 查询参数匹配类型
 */
export type QueryParamMatchType = 'Exact' | 'RegularExpression';

/**
 * HTTPMethod - HTTP 请求方法
 */
export type HTTPMethod =
  | 'GET'
  | 'HEAD'
  | 'POST'
  | 'PUT'
  | 'DELETE'
  | 'CONNECT'
  | 'OPTIONS'
  | 'TRACE'
  | 'PATCH';

/**
 * HTTPPathMatch - 路径匹配
 */
export interface HTTPPathMatch {
  /** 匹配类型，默认 "PathPrefix" */
  type?: PathMatchType;
  
  /** 路径值，默认 "/" */
  value?: string;
}

/**
 * HTTPHeaderMatch - Header 匹配
 */
export interface HTTPHeaderMatch {
  /** 匹配类型，默认 "Exact" */
  type?: HeaderMatchType;
  
  /** Header 名称（必填，大小写不敏感） */
  name: string;
  
  /** Header 值（必填） */
  value: string;
}

/**
 * HTTPQueryParamMatch - 查询参数匹配
 */
export interface HTTPQueryParamMatch {
  /** 匹配类型，默认 "Exact" */
  type?: QueryParamMatchType;
  
  /** 参数名称（必填） */
  name: string;
  
  /** 参数值（必填） */
  value: string;
}

/**
 * HTTPRouteMatch - HTTP 路由匹配条件
 */
export interface HTTPRouteMatch {
  /** 路径匹配 */
  path?: HTTPPathMatch;
  
  /** Header 匹配列表 */
  headers?: HTTPHeaderMatch[];
  
  /** 查询参数匹配列表 */
  queryParams?: HTTPQueryParamMatch[];
  
  /** HTTP 请求方法 */
  method?: HTTPMethod;
}

// ============================================
// HTTPRoute 过滤器类型
// ============================================

/**
 * HTTPRouteFilterType - 过滤器类型
 */
export type HTTPRouteFilterType =
  | 'RequestHeaderModifier'
  | 'ResponseHeaderModifier'
  | 'RequestRedirect'
  | 'URLRewrite'
  | 'RequestMirror'
  | 'ExtensionRef';

/**
 * HTTPRequestHeaderFilter - 请求头/响应头修改过滤器
 */
export interface HTTPRequestHeaderFilter {
  /** 设置/替换 Header（如果存在则替换，不存在则添加） */
  set?: HTTPHeader[];
  
  /** 添加 Header（允许重复） */
  add?: HTTPHeader[];
  
  /** 删除 Header（只需 name） */
  remove?: string[];
}

/**
 * HTTPPathModifierType - 路径修改类型
 */
export type HTTPPathModifierType = 'ReplaceFullPath' | 'ReplacePrefixMatch';

/**
 * HTTPPathModifier - 路径修改
 */
export interface HTTPPathModifier {
  /** 修改类型（必填） */
  type: HTTPPathModifierType;
  
  /** 替换完整路径（当 type 为 ReplaceFullPath 时使用） */
  replaceFullPath?: string;
  
  /** 替换路径前缀（当 type 为 ReplacePrefixMatch 时使用） */
  replacePrefixMatch?: string;
}

/**
 * HTTPRequestRedirectFilter - 请求重定向过滤器
 */
export interface HTTPRequestRedirectFilter {
  /** URL Scheme（http 或 https） */
  scheme?: 'http' | 'https';
  
  /** 主机名 */
  hostname?: PreciseHostname;
  
  /** 路径修改 */
  path?: HTTPPathModifier;
  
  /** 端口号（1-65535） */
  port?: number;
  
  /** 状态码（301 或 302，默认 302） */
  statusCode?: 301 | 302;
}

/**
 * HTTPURLRewriteFilter - URL 重写过滤器
 */
export interface HTTPURLRewriteFilter {
  /** 主机名 */
  hostname?: PreciseHostname;
  
  /** 路径修改 */
  path?: HTTPPathModifier;
}

/**
 * HTTPRequestMirrorFilter - 请求镜像过滤器
 */
export interface HTTPRequestMirrorFilter {
  /** 后端服务引用（必填） */
  backendRef: BackendObjectReference;
}

/**
 * HTTPRouteFilter - HTTP 路由过滤器
 */
export interface HTTPRouteFilter {
  /** 过滤器类型（必填） */
  type: HTTPRouteFilterType;
  
  /** 请求头修改 */
  requestHeaderModifier?: HTTPRequestHeaderFilter;
  
  /** 响应头修改 */
  responseHeaderModifier?: HTTPRequestHeaderFilter;
  
  /** 请求重定向 */
  requestRedirect?: HTTPRequestRedirectFilter;
  
  /** URL 重写 */
  urlRewrite?: HTTPURLRewriteFilter;
  
  /** 请求镜像 */
  requestMirror?: HTTPRequestMirrorFilter;
  
  /** 扩展引用 */
  extensionRef?: LocalObjectReference;
}

// ============================================
// HTTPRoute 超时配置
// ============================================

/**
 * HTTPRouteTimeouts - 超时配置
 */
export interface HTTPRouteTimeouts {
  /** 请求超时（Duration，如 "30s", "1m"） */
  request?: Duration;
  
  /** 后端请求超时（Duration） */
  backendRequest?: Duration;
}

// ============================================
// HTTPRoute Rule - 路由规则
// ============================================

/**
 * HTTPRouteRule - HTTP 路由规则
 */
export interface HTTPRouteRule {
  /** 匹配条件列表 */
  matches?: HTTPRouteMatch[];
  
  /** 过滤器列表 */
  filters?: HTTPRouteFilter[];
  
  /** 后端服务列表 */
  backendRefs?: BackendRef[];
  
  /** 超时配置 */
  timeouts?: HTTPRouteTimeouts;
}

// ============================================
// HTTPRoute Spec
// ============================================

/**
 * HTTPRouteSpec - HTTPRoute 规格
 */
export interface HTTPRouteSpec {
  /** 父引用（Gateway）列表（必填，至少一个） */
  parentRefs: ParentReference[];
  
  /** 主机名列表（可选） */
  hostnames?: Hostname[];
  
  /** 路由规则列表（可选） */
  rules?: HTTPRouteRule[];
}

// ============================================
// HTTPRoute Status
// ============================================

/**
 * RouteParentStatus - 路由父引用状态
 */
export interface RouteParentStatus {
  parentRef: ParentReference;
  controllerName: string;
  conditions?: import('./common').Condition[];
}

/**
 * HTTPRouteStatus - HTTPRoute 状态（只读）
 */
export interface HTTPRouteStatus {
  parents?: RouteParentStatus[];
}

// ============================================
// HTTPRoute Resource
// ============================================

/**
 * HTTPRoute - 完整的 HTTPRoute 资源
 */
export interface HTTPRoute extends K8sResource<HTTPRouteSpec> {
  apiVersion: 'gateway.networking.k8s.io/v1';
  kind: 'HTTPRoute';
  metadata: K8sObjectMeta;
  spec: HTTPRouteSpec;
  status?: HTTPRouteStatus;
}

