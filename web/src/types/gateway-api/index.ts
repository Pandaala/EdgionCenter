/**
 * Gateway API 类型统一导出
 */

// 通用类型
export type {
  K8sObjectMeta,
  K8sResource,
  Hostname,
  PreciseHostname,
  Duration,
  Condition,
} from './common';

// 后端引用类型
export type {
  ParentReference,
  BackendRef,
  BackendObjectReference,
  LocalObjectReference,
  HTTPHeader,
} from './backend';

// HTTPRoute 类型
export type {
  // 匹配类型
  PathMatchType,
  HeaderMatchType,
  QueryParamMatchType,
  HTTPMethod,
  HTTPPathMatch,
  HTTPHeaderMatch,
  HTTPQueryParamMatch,
  HTTPRouteMatch,
  
  // 过滤器类型
  HTTPRouteFilterType,
  HTTPRequestHeaderFilter,
  HTTPPathModifierType,
  HTTPPathModifier,
  HTTPRequestRedirectFilter,
  HTTPURLRewriteFilter,
  HTTPRequestMirrorFilter,
  HTTPRouteFilter,
  
  // 超时配置
  HTTPRouteTimeouts,
  
  // 路由规则
  HTTPRouteRule,
  
  // Spec 和 Status
  HTTPRouteSpec,
  RouteParentStatus,
  HTTPRouteStatus,
  
  // 完整资源
  HTTPRoute,
} from './httproute';

