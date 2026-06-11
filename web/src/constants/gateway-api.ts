/**
 * Gateway API 常量定义
 * 包含验证规则、默认值、正则表达式等
 */

// ============================================
// 正则表达式 - 验证规则
// ============================================

/**
 * DNS-1123 子域名验证（用于 metadata.name, namespace）
 * - 只能包含小写字母、数字、连字符(-)、点(.)
 * - 必须以字母或数字开头和结尾
 * - 最多 253 字符
 */
export const DNS1123_SUBDOMAIN_PATTERN = /^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$/;
export const DNS1123_SUBDOMAIN_MAX_LENGTH = 253;

/**
 * DNS-1123 标签验证（用于 labels 的 key）
 * - 只能包含小写字母、数字、连字符(-)
 * - 必须以字母或数字开头和结尾
 * - 最多 63 字符
 */
export const DNS1123_LABEL_PATTERN = /^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/;
export const DNS1123_LABEL_MAX_LENGTH = 63;

/**
 * Hostname 验证（RFC 1123）
 * - 支持通配符前缀 *.example.com
 * - 最多 253 字符
 */
export const HOSTNAME_PATTERN = /^(\*\.)?[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$/;
export const HOSTNAME_MAX_LENGTH = 253;

/**
 * HTTP Header 名称验证（RFC 7230）
 * - 允许的字符：字母、数字、特殊符号
 */
export const HTTP_HEADER_NAME_PATTERN = /^[a-zA-Z0-9!#$%&'*+\-.^_`|~]+$/;

/**
 * 端口号范围
 */
export const PORT_MIN = 1;
export const PORT_MAX = 65535;

/**
 * BackendRef weight 范围
 */
export const WEIGHT_MIN = 0;
export const WEIGHT_MAX = 1000000;

// ============================================
// 枚举类型
// ============================================

/**
 * HTTP 请求方法
 */
export const HTTP_METHODS = [
  'GET',
  'HEAD',
  'POST',
  'PUT',
  'DELETE',
  'CONNECT',
  'OPTIONS',
  'TRACE',
  'PATCH',
] as const;

export type HTTPMethod = typeof HTTP_METHODS[number];

/**
 * 路径匹配类型
 */
export const PATH_MATCH_TYPES = ['Exact', 'PathPrefix', 'RegularExpression'] as const;
export type PathMatchType = typeof PATH_MATCH_TYPES[number];

/**
 * Header/QueryParam 匹配类型
 */
export const HEADER_MATCH_TYPES = ['Exact', 'RegularExpression'] as const;
export type HeaderMatchType = typeof HEADER_MATCH_TYPES[number];

/**
 * 路径修改类型
 */
export const PATH_MODIFIER_TYPES = ['ReplaceFullPath', 'ReplacePrefixMatch'] as const;
export type PathModifierType = typeof PATH_MODIFIER_TYPES[number];

/**
 * HTTP 过滤器类型
 */
export const HTTP_FILTER_TYPES = [
  'RequestHeaderModifier',
  'ResponseHeaderModifier',
  'RequestRedirect',
  'URLRewrite',
  'RequestMirror',
  'ExtensionRef',
] as const;
export type HTTPFilterType = typeof HTTP_FILTER_TYPES[number];

/**
 * HTTP Scheme
 */
export const HTTP_SCHEMES = ['http', 'https'] as const;
export type HTTPScheme = typeof HTTP_SCHEMES[number];

/**
 * 重定向状态码
 */
export const REDIRECT_STATUS_CODES = [301, 302] as const;
export type RedirectStatusCode = typeof REDIRECT_STATUS_CODES[number];

// ============================================
// 默认值
// ============================================

/**
 * 字段默认值定义
 */
export const DEFAULT_VALUES = {
  // Gateway API 版本
  apiVersion: 'gateway.networking.k8s.io/v1',
  
  // HTTPRoute kind
  httpRouteKind: 'HTTPRoute',
  
  // 默认命名空间
  defaultNamespace: 'default',
  
  // ParentReference 默认值
  parentRef: {
    group: 'gateway.networking.k8s.io',
    kind: 'Gateway',
  },
  
  // BackendRef 默认值
  backendRef: {
    group: '',
    kind: 'Service',
    weight: 1,
  },
  
  // HTTPPathMatch 默认值
  pathMatch: {
    type: 'PathPrefix' as PathMatchType,
    value: '/',
  },
  
  // HTTPHeaderMatch/HTTPQueryParamMatch 默认值
  matchType: {
    type: 'Exact' as HeaderMatchType,
  },
  
  // HTTPRequestRedirectFilter 默认值
  redirect: {
    statusCode: 302 as RedirectStatusCode,
  },
} as const;

// ============================================
// 验证错误消息（中英双语）
// ============================================

export const VALIDATION_MESSAGES = {
  required: {
    zh: '此字段为必填项',
    en: 'This field is required',
  },
  dns1123Subdomain: {
    zh: '必须符合 DNS-1123 规范（小写字母、数字、连字符、点）',
    en: 'Must be a valid DNS-1123 subdomain (lowercase letters, numbers, hyphens, dots)',
  },
  dns1123Label: {
    zh: '必须符合 DNS-1123 标签规范（小写字母、数字、连字符）',
    en: 'Must be a valid DNS-1123 label (lowercase letters, numbers, hyphens)',
  },
  hostname: {
    zh: '必须是有效的主机名（支持通配符前缀 *）',
    en: 'Must be a valid hostname (wildcard prefix * is supported)',
  },
  httpHeaderName: {
    zh: '必须是有效的 HTTP Header 名称',
    en: 'Must be a valid HTTP header name',
  },
  portRange: {
    zh: `端口号必须在 ${PORT_MIN}-${PORT_MAX} 范围内`,
    en: `Port must be between ${PORT_MIN} and ${PORT_MAX}`,
  },
  weightRange: {
    zh: `权重必须在 ${WEIGHT_MIN}-${WEIGHT_MAX} 范围内`,
    en: `Weight must be between ${WEIGHT_MIN} and ${WEIGHT_MAX}`,
  },
  maxLength: (max: number) => ({
    zh: `最多 ${max} 个字符`,
    en: `Maximum ${max} characters`,
  }),
  minItems: (min: number) => ({
    zh: `至少需要 ${min} 个项目`,
    en: `At least ${min} item(s) required`,
  }),
} as const;

/**
 * 获取验证错误消息（根据当前语言）
 */
export function getValidationMessage(
  key: keyof typeof VALIDATION_MESSAGES,
  lang: 'zh' | 'en' = 'zh'
): string {
  const message = VALIDATION_MESSAGES[key];
  if (typeof message === 'function') {
    throw new Error('Please call the function with required parameters');
  }
  return message[lang];
}

// ============================================
// UI 显示文本（中英双语）
// ============================================

export const UI_LABELS = {
  httpRoute: {
    zh: 'HTTP 路由',
    en: 'HTTP Route',
  },
  metadata: {
    zh: '基础信息',
    en: 'Basic Info',
  },
  parentRefs: {
    zh: 'Gateway 引用',
    en: 'Gateway References',
  },
  hostnames: {
    zh: '主机名',
    en: 'Hostnames',
  },
  rules: {
    zh: '路由规则',
    en: 'Rules',
  },
  matches: {
    zh: '匹配条件',
    en: 'Matches',
  },
  filters: {
    zh: '过滤器',
    en: 'Filters',
  },
  backendRefs: {
    zh: '后端服务',
    en: 'Backend Services',
  },
} as const;

