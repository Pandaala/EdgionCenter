/**
 * HTTPRoute 工具函数
 */

import * as yaml from 'js-yaml';
import type { HTTPRoute } from '@/types/gateway-api';
import { DEFAULT_VALUES } from '@/constants/gateway-api';

/**
 * 默认的 HTTPRoute YAML 模板
 */
export const DEFAULT_HTTPROUTE_YAML = `apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: example-route
  namespace: default
spec:
  parentRefs:
    - name: example-gateway
  hostnames:
    - "example.com"
  rules:
    - matches:
        - path:
            type: PathPrefix
            value: /
      backendRefs:
        - name: example-service
          port: 80
`;

/**
 * 创建空的 HTTPRoute 对象（带默认值）
 */
export function createEmptyHTTPRoute(): HTTPRoute {
  return {
    apiVersion: DEFAULT_VALUES.apiVersion,
    kind: DEFAULT_VALUES.httpRouteKind,
    metadata: {
      name: '',
      namespace: DEFAULT_VALUES.defaultNamespace,
      labels: {},
      annotations: {},
    },
    spec: {
      parentRefs: [
        {
          group: DEFAULT_VALUES.parentRef.group,
          kind: DEFAULT_VALUES.parentRef.kind,
          name: '',
        },
      ],
      hostnames: [],
      rules: [
        {
          matches: [
            {
              path: {
                type: DEFAULT_VALUES.pathMatch.type,
                value: DEFAULT_VALUES.pathMatch.value,
              },
            },
          ],
          backendRefs: [
            {
              group: DEFAULT_VALUES.backendRef.group,
              kind: DEFAULT_VALUES.backendRef.kind,
              name: '',
              port: 80,
              weight: DEFAULT_VALUES.backendRef.weight,
            },
          ],
        },
      ],
    },
  };
}

/**
 * 规范化 HTTPRoute 对象（填充默认值）
 */
export function normalizeHTTPRoute(route: Partial<HTTPRoute>): HTTPRoute {
  return {
    apiVersion: route.apiVersion || DEFAULT_VALUES.apiVersion,
    kind: route.kind || DEFAULT_VALUES.httpRouteKind,
    metadata: {
      name: route.metadata?.name || '',
      namespace: route.metadata?.namespace || DEFAULT_VALUES.defaultNamespace,
      labels: route.metadata?.labels || {},
      annotations: route.metadata?.annotations || {},
      creationTimestamp: route.metadata?.creationTimestamp,
      resourceVersion: route.metadata?.resourceVersion,
      uid: route.metadata?.uid,
    },
    spec: {
      parentRefs: (route.spec?.parentRefs || []).map((ref) => ({
        group: ref.group || DEFAULT_VALUES.parentRef.group,
        kind: ref.kind || DEFAULT_VALUES.parentRef.kind,
        namespace: ref.namespace || route.metadata?.namespace || DEFAULT_VALUES.defaultNamespace,
        name: ref.name,
        sectionName: ref.sectionName,
        port: ref.port,
      })),
      hostnames: route.spec?.hostnames || [],
      rules: (route.spec?.rules || []).map((rule) => ({
        matches: rule.matches?.map((match) => ({
          path: match.path
            ? {
                type: match.path.type || DEFAULT_VALUES.pathMatch.type,
                value: match.path.value || DEFAULT_VALUES.pathMatch.value,
              }
            : undefined,
          headers: match.headers?.map((h) => ({
            type: h.type || DEFAULT_VALUES.matchType.type,
            name: h.name,
            value: h.value,
          })),
          queryParams: match.queryParams?.map((q) => ({
            type: q.type || DEFAULT_VALUES.matchType.type,
            name: q.name,
            value: q.value,
          })),
          method: match.method,
        })),
        filters: rule.filters,
        backendRefs: rule.backendRefs?.map((ref) => ({
          group: ref.group || DEFAULT_VALUES.backendRef.group,
          kind: ref.kind || DEFAULT_VALUES.backendRef.kind,
          namespace: ref.namespace || route.metadata?.namespace || DEFAULT_VALUES.defaultNamespace,
          name: ref.name,
          port: ref.port,
          weight: ref.weight ?? DEFAULT_VALUES.backendRef.weight,
        })),
        timeouts: rule.timeouts,
      })),
    },
    status: route.status,
  };
}

/**
 * HTTPRoute 转 YAML 字符串
 */
export function httpRouteToYAML(route: HTTPRoute): string {
  return yaml.dump(route, {
    lineWidth: -1,
    noRefs: true,
    quotingType: '"',
    forceQuotes: false,
  });
}

/**
 * YAML 字符串转 HTTPRoute
 */
export function yamlToHTTPRoute(yamlString: string): HTTPRoute {
  return yaml.load(yamlString) as HTTPRoute;
}

/**
 * 验证 HTTPRoute 对象是否完整（基本验证）
 */
export function isHTTPRouteValid(route: Partial<HTTPRoute>): boolean {
  return !!(
    route.metadata?.name &&
    route.metadata?.namespace &&
    route.spec?.parentRefs &&
    route.spec.parentRefs.length > 0
  );
}

/**
 * 获取 HTTPRoute 的摘要信息（用于列表显示）
 */
export function getHTTPRouteSummary(route: HTTPRoute): {
  name: string;
  namespace: string;
  parentRefs: string[];
  hostnames: string[];
  rulesCount: number;
} {
  return {
    name: route.metadata.name,
    namespace: route.metadata.namespace || DEFAULT_VALUES.defaultNamespace,
    parentRefs: route.spec.parentRefs.map((ref) => ref.name),
    hostnames: route.spec.hostnames || [],
    rulesCount: route.spec.rules?.length || 0,
  };
}

