/**
 * HTTPRoute Zod Schema 验证
 */

import { z } from 'zod';
import {
  hostnameSchema,
  portSchema,
  k8sObjectMetaSchema,
  parentReferenceSchema,
  backendRefSchema,
  backendObjectReferenceSchema,
  localObjectReferenceSchema,
  httpHeaderSchema,
} from './common';
import {
  VALIDATION_MESSAGES,
} from '@/constants/gateway-api';

// ============================================
// HTTPRoute Match Schema
// ============================================

/**
 * HTTPPathMatch Schema
 */
export const httpPathMatchSchema = z.object({
  type: z.enum(['Exact', 'PathPrefix', 'RegularExpression']).optional(),
  value: z.string().optional(),
});

/**
 * HTTPHeaderMatch Schema
 */
export const httpHeaderMatchSchema = z.object({
  type: z.enum(['Exact', 'RegularExpression']).optional(),
  name: z.string().min(1, VALIDATION_MESSAGES.required.zh),
  value: z.string(),
});

/**
 * HTTPQueryParamMatch Schema
 */
export const httpQueryParamMatchSchema = z.object({
  type: z.enum(['Exact', 'RegularExpression']).optional(),
  name: z.string().min(1, VALIDATION_MESSAGES.required.zh),
  value: z.string(),
});

/**
 * HTTPRouteMatch Schema
 */
export const httpRouteMatchSchema = z.object({
  path: httpPathMatchSchema.optional(),
  headers: z.array(httpHeaderMatchSchema).optional(),
  queryParams: z.array(httpQueryParamMatchSchema).optional(),
  method: z.enum(['GET', 'HEAD', 'POST', 'PUT', 'DELETE', 'CONNECT', 'OPTIONS', 'TRACE', 'PATCH']).optional(),
});

// ============================================
// HTTPRoute Filter Schema
// ============================================

/**
 * HTTPRequestHeaderFilter Schema
 */
export const httpRequestHeaderFilterSchema = z.object({
  set: z.array(httpHeaderSchema).optional(),
  add: z.array(httpHeaderSchema).optional(),
  remove: z.array(z.string()).optional(),
});

/**
 * HTTPPathModifier Schema
 */
export const httpPathModifierSchema = z.object({
  type: z.enum(['ReplaceFullPath', 'ReplacePrefixMatch']),
  replaceFullPath: z.string().optional(),
  replacePrefixMatch: z.string().optional(),
});

/**
 * HTTPRequestRedirectFilter Schema
 */
export const httpRequestRedirectFilterSchema = z.object({
  scheme: z.enum(['http', 'https']).optional(),
  hostname: hostnameSchema.optional(),
  path: httpPathModifierSchema.optional(),
  port: portSchema.optional(),
  statusCode: z.union([z.literal(301), z.literal(302)]).optional(),
});

/**
 * HTTPURLRewriteFilter Schema
 */
export const httpURLRewriteFilterSchema = z.object({
  hostname: hostnameSchema.optional(),
  path: httpPathModifierSchema.optional(),
});

/**
 * HTTPRequestMirrorFilter Schema
 */
export const httpRequestMirrorFilterSchema = z.object({
  backendRef: backendObjectReferenceSchema,
});

/**
 * HTTPRouteFilter Schema
 */
export const httpRouteFilterSchema = z.object({
  type: z.enum(['RequestHeaderModifier', 'ResponseHeaderModifier', 'RequestRedirect', 'URLRewrite', 'RequestMirror', 'ExtensionRef']),
  requestHeaderModifier: httpRequestHeaderFilterSchema.optional(),
  responseHeaderModifier: httpRequestHeaderFilterSchema.optional(),
  requestRedirect: httpRequestRedirectFilterSchema.optional(),
  urlRewrite: httpURLRewriteFilterSchema.optional(),
  requestMirror: httpRequestMirrorFilterSchema.optional(),
  extensionRef: localObjectReferenceSchema.optional(),
});

// ============================================
// HTTPRoute Timeouts Schema
// ============================================

/**
 * HTTPRouteTimeouts Schema
 */
export const httpRouteTimeoutsSchema = z.object({
  request: z.string().optional(),
  backendRequest: z.string().optional(),
});

// ============================================
// HTTPRoute Rule Schema
// ============================================

/**
 * HTTPRouteRule Schema
 */
export const httpRouteRuleSchema = z.object({
  matches: z.array(httpRouteMatchSchema).optional(),
  filters: z.array(httpRouteFilterSchema).optional(),
  backendRefs: z.array(backendRefSchema).optional(),
  timeouts: httpRouteTimeoutsSchema.optional(),
});

// ============================================
// HTTPRoute Spec Schema
// ============================================

/**
 * HTTPRouteSpec Schema
 */
export const httpRouteSpecSchema = z.object({
  parentRefs: z
    .array(parentReferenceSchema)
    .min(1, VALIDATION_MESSAGES.minItems(1).zh),
  hostnames: z.array(hostnameSchema).optional(),
  rules: z.array(httpRouteRuleSchema).optional(),
});

// ============================================
// HTTPRoute Schema
// ============================================

/**
 * HTTPRoute 完整 Schema
 */
export const httpRouteSchema = z.object({
  apiVersion: z.literal('gateway.networking.k8s.io/v1'),
  kind: z.literal('HTTPRoute'),
  metadata: k8sObjectMetaSchema,
  spec: httpRouteSpecSchema,
  status: z.any().optional(),
});

/**
 * HTTPRoute 类型（从 Schema 推导）
 */
export type HTTPRouteSchemaType = z.infer<typeof httpRouteSchema>;

