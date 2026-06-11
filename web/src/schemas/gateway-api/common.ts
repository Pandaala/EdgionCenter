/**
 * Gateway API 通用 Zod Schema 验证
 */

import { z } from 'zod';
import {
  DNS1123_SUBDOMAIN_PATTERN,
  DNS1123_SUBDOMAIN_MAX_LENGTH,
  DNS1123_LABEL_PATTERN,
  DNS1123_LABEL_MAX_LENGTH,
  HOSTNAME_PATTERN,
  HOSTNAME_MAX_LENGTH,
  HTTP_HEADER_NAME_PATTERN,
  PORT_MIN,
  PORT_MAX,
  WEIGHT_MIN,
  WEIGHT_MAX,
  VALIDATION_MESSAGES,
} from '@/constants/gateway-api';

// ============================================
// 基础验证 Schema
// ============================================

/**
 * DNS-1123 子域名验证
 */
export const dns1123SubdomainSchema = z
  .string()
  .min(1, VALIDATION_MESSAGES.required.zh)
  .max(DNS1123_SUBDOMAIN_MAX_LENGTH, VALIDATION_MESSAGES.maxLength(DNS1123_SUBDOMAIN_MAX_LENGTH).zh)
  .regex(DNS1123_SUBDOMAIN_PATTERN, VALIDATION_MESSAGES.dns1123Subdomain.zh);

/**
 * DNS-1123 标签验证
 */
export const dns1123LabelSchema = z
  .string()
  .min(1, VALIDATION_MESSAGES.required.zh)
  .max(DNS1123_LABEL_MAX_LENGTH, VALIDATION_MESSAGES.maxLength(DNS1123_LABEL_MAX_LENGTH).zh)
  .regex(DNS1123_LABEL_PATTERN, VALIDATION_MESSAGES.dns1123Label.zh);

/**
 * Hostname 验证
 */
export const hostnameSchema = z
  .string()
  .min(1, VALIDATION_MESSAGES.required.zh)
  .max(HOSTNAME_MAX_LENGTH, VALIDATION_MESSAGES.maxLength(HOSTNAME_MAX_LENGTH).zh)
  .regex(HOSTNAME_PATTERN, VALIDATION_MESSAGES.hostname.zh);

/**
 * HTTP Header 名称验证
 */
export const httpHeaderNameSchema = z
  .string()
  .min(1, VALIDATION_MESSAGES.required.zh)
  .regex(HTTP_HEADER_NAME_PATTERN, VALIDATION_MESSAGES.httpHeaderName.zh);

/**
 * 端口号验证
 */
export const portSchema = z
  .number()
  .int()
  .min(PORT_MIN, VALIDATION_MESSAGES.portRange.zh)
  .max(PORT_MAX, VALIDATION_MESSAGES.portRange.zh);

/**
 * Weight 验证
 */
export const weightSchema = z
  .number()
  .int()
  .min(WEIGHT_MIN, VALIDATION_MESSAGES.weightRange.zh)
  .max(WEIGHT_MAX, VALIDATION_MESSAGES.weightRange.zh);

// ============================================
// Kubernetes 通用 Schema
// ============================================

/**
 * K8sObjectMeta Schema
 */
export const k8sObjectMetaSchema = z.object({
  name: dns1123SubdomainSchema,
  namespace: dns1123SubdomainSchema.optional(),
  labels: z.record(z.string(), z.string()).optional(),
  annotations: z.record(z.string(), z.string()).optional(),
  creationTimestamp: z.string().optional(),
  resourceVersion: z.string().optional(),
  uid: z.string().optional(),
});

// ============================================
// Gateway API 通用 Schema
// ============================================

/**
 * ParentReference Schema
 */
export const parentReferenceSchema = z.object({
  group: z.string().optional(),
  kind: z.string().optional(),
  namespace: dns1123SubdomainSchema.optional(),
  name: dns1123SubdomainSchema,
  sectionName: z.string().optional(),
  port: portSchema.optional(),
});

/**
 * BackendRef Schema
 */
export const backendRefSchema = z.object({
  group: z.string().optional(),
  kind: z.string().optional(),
  namespace: dns1123SubdomainSchema.optional(),
  name: dns1123SubdomainSchema,
  port: portSchema.optional(),
  weight: weightSchema.optional(),
});

/**
 * BackendObjectReference Schema
 */
export const backendObjectReferenceSchema = z.object({
  group: z.string().optional(),
  kind: z.string().optional(),
  namespace: dns1123SubdomainSchema.optional(),
  name: dns1123SubdomainSchema,
  port: portSchema.optional(),
});

/**
 * LocalObjectReference Schema
 */
export const localObjectReferenceSchema = z.object({
  group: z.string().min(1, VALIDATION_MESSAGES.required.zh),
  kind: z.string().min(1, VALIDATION_MESSAGES.required.zh),
  name: dns1123SubdomainSchema,
});

/**
 * HTTPHeader Schema
 */
export const httpHeaderSchema = z.object({
  name: httpHeaderNameSchema,
  value: z.string(),
});

