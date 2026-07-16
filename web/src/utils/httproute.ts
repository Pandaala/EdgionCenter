/**
 * HTTPRoute 工具函数
 */

import * as yaml from 'js-yaml';
import type { HTTPRoute } from '@/types/gateway-api';
import { DEFAULT_VALUES } from '@/constants/gateway-api';
import { buildMutationDocument } from './resource-document';

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
export function normalizeHTTPRoute(raw: unknown): HTTPRoute {
  if (!raw || typeof raw !== 'object') throw new Error('HTTPRoute must be an object');
  const route = raw as HTTPRoute;
  if (route.kind !== 'HTTPRoute') throw new Error('Expected HTTPRoute kind');
  if (!route.metadata || !route.spec) throw new Error('HTTPRoute metadata and spec are required');
  return structuredClone(route);
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
  return normalizeHTTPRoute(yaml.load(yamlString));
}

export function toHTTPRouteMutationDocument(
  route: HTTPRoute,
  mode: 'create' | 'update',
): Record<string, unknown> {
  validateHTTPRouteForMutation(route);
  return buildMutationDocument(route, { resourceKind: 'httproute', mode });
}

const isHTTPDelegationRef = (ref: any) =>
  ref?.group === 'gateway.networking.k8s.io' && ref?.kind === 'HTTPRoute';

export function validateHTTPRouteForMutation(route: HTTPRoute): void {
  for (const [ruleIndex, rule] of (route.spec.rules || []).entries()) {
    const validateFilters = (filters: any[], location: string) => {
      for (const filter of filters) {
        const path = filter.requestRedirect?.path || filter.urlRewrite?.path
        if (path?.replaceFullPath !== undefined && path?.replacePrefixMatch !== undefined) {
          throw new Error(`${location} path modifier cannot set both replaceFullPath and replacePrefixMatch`)
        }
        if (filter.type === 'ExternalAuth') {
          const target = filter.externalAuth?.target || {}
          const hasService = ['name', 'namespace', 'port', 'kind', 'group'].some((key) => target[key] !== undefined && target[key] !== '')
          if (target.url && hasService) throw new Error(`${location} ExternalAuth target cannot combine url and Service fields`)
          if (!target.url && !target.name) throw new Error(`${location} ExternalAuth target requires url or name`)
        }
      }
    }
    validateFilters(rule.filters || [], `rules[${ruleIndex}].filters`)
    const ruleTypes = new Set((rule.filters || []).map((filter) => filter.type));
    if (ruleTypes.has('RequestRedirect') && ruleTypes.has('URLRewrite')) {
      throw new Error(`rules[${ruleIndex}] cannot combine RequestRedirect and URLRewrite`);
    }
    for (const [backendIndex, backend] of (rule.backendRefs || []).entries()) {
      validateFilters(backend.filters || [], `rules[${ruleIndex}].backendRefs[${backendIndex}].filters`)
      const backendTypes = new Set((backend.filters || []).map((filter) => filter.type));
      if (backendTypes.has('RequestRedirect') && backendTypes.has('URLRewrite')) {
        throw new Error(`rules[${ruleIndex}].backendRefs[${backendIndex}] cannot combine RequestRedirect and URLRewrite`);
      }
      if ((ruleTypes.has('RequestRedirect') && backendTypes.has('URLRewrite')) ||
          (ruleTypes.has('URLRewrite') && backendTypes.has('RequestRedirect'))) {
        throw new Error(`rules[${ruleIndex}] cannot combine RequestRedirect and URLRewrite across rule and backend filters`);
      }
    }
    if ((rule.backendRefs || []).some(isHTTPDelegationRef)) {
      if (rule.backendRefs?.length !== 1) throw new Error(`rules[${ruleIndex}] delegation requires exactly one backendRef`);
      const ref = rule.backendRefs[0];
      if (ref.port !== undefined) throw new Error(`rules[${ruleIndex}] delegation backendRef must not set port`);
      if (ref.filters !== undefined) throw new Error(`rules[${ruleIndex}] delegation backendRef must not set filters`);
      for (const match of rule.matches || []) {
        if (match.path?.type && match.path.type !== 'PathPrefix') {
          throw new Error(`rules[${ruleIndex}] delegation matches must use PathPrefix`);
        }
      }
    }
    for (const code of rule.retry?.codes || []) {
      if (!Number.isInteger(code) || code < 100 || code > 599) {
        throw new Error(`rules[${ruleIndex}].retry.codes must contain HTTP status codes from 100 through 599`);
      }
    }
  }
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
