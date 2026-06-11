/**
 * 验证工具函数
 */

import {
  DNS1123_SUBDOMAIN_PATTERN,
  DNS1123_LABEL_PATTERN,
  HOSTNAME_PATTERN,
  HTTP_HEADER_NAME_PATTERN,
  PORT_MIN,
  PORT_MAX,
  WEIGHT_MIN,
  WEIGHT_MAX,
} from '@/constants/gateway-api';

/**
 * 验证 DNS-1123 子域名
 */
export function isValidDNS1123Subdomain(value: string): boolean {
  return DNS1123_SUBDOMAIN_PATTERN.test(value);
}

/**
 * 验证 DNS-1123 标签
 */
export function isValidDNS1123Label(value: string): boolean {
  return DNS1123_LABEL_PATTERN.test(value);
}

/**
 * 验证 Hostname
 */
export function isValidHostname(value: string): boolean {
  return HOSTNAME_PATTERN.test(value);
}

/**
 * 验证 HTTP Header 名称
 */
export function isValidHTTPHeaderName(value: string): boolean {
  return HTTP_HEADER_NAME_PATTERN.test(value);
}

/**
 * 验证端口号
 */
export function isValidPort(value: number): boolean {
  return Number.isInteger(value) && value >= PORT_MIN && value <= PORT_MAX;
}

/**
 * 验证权重
 */
export function isValidWeight(value: number): boolean {
  return Number.isInteger(value) && value >= WEIGHT_MIN && value <= WEIGHT_MAX;
}

/**
 * 格式化验证错误消息
 */
export function formatValidationError(error: any): string {
  if (error.issues && Array.isArray(error.issues)) {
    return error.issues.map((issue: any) => issue.message).join('; ');
  }
  return error.message || '验证失败';
}

