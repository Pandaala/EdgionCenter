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
 * Canonical GEP-2257 duration grammar used by Edgion resource schemas.
 */
export const GEP2257_DURATION_PATTERN = /^([0-9]{1,5}(h|m|s|ms)){1,4}$/;

export interface Gep2257DurationRange {
  minimumMilliseconds?: number;
  maximumMilliseconds?: number;
  minimumInclusive?: boolean;
  maximumInclusive?: boolean;
}

/**
 * Return whether a value matches Edgion's canonical GEP-2257 duration grammar.
 */
export function isValidGep2257Duration(value: string): boolean {
  return GEP2257_DURATION_PATTERN.test(value);
}

/**
 * Convert a valid GEP-2257 duration to milliseconds without rounding.
 */
export function parseGep2257DurationMilliseconds(value: string): number | null {
  if (!isValidGep2257Duration(value)) {
    return null;
  }

  const segmentPattern = /([0-9]{1,5})(ms|h|m|s)/g;
  const multipliers: Record<string, number> = {
    h: 3_600_000,
    m: 60_000,
    s: 1_000,
    ms: 1,
  };
  let totalMilliseconds = 0;

  for (const match of value.matchAll(segmentPattern)) {
    totalMilliseconds += Number(match[1]) * multipliers[match[2]];
  }

  return Number.isSafeInteger(totalMilliseconds) ? totalMilliseconds : null;
}

/**
 * Check a valid GEP-2257 duration against optional millisecond boundaries.
 * Boundaries are inclusive by default.
 */
export function isGep2257DurationInRange(
  value: string,
  range: Gep2257DurationRange,
): boolean {
  const totalMilliseconds = parseGep2257DurationMilliseconds(value);
  if (totalMilliseconds === null) {
    return false;
  }

  const {
    minimumMilliseconds,
    maximumMilliseconds,
    minimumInclusive = true,
    maximumInclusive = true,
  } = range;
  const boundaries = [minimumMilliseconds, maximumMilliseconds].filter(
    (boundary): boundary is number => boundary !== undefined,
  );
  if (boundaries.some((boundary) => !Number.isSafeInteger(boundary) || boundary < 0)) {
    return false;
  }

  if (
    minimumMilliseconds !== undefined
    && (minimumInclusive
      ? totalMilliseconds < minimumMilliseconds
      : totalMilliseconds <= minimumMilliseconds)
  ) {
    return false;
  }
  if (
    maximumMilliseconds !== undefined
    && (maximumInclusive
      ? totalMilliseconds > maximumMilliseconds
      : totalMilliseconds >= maximumMilliseconds)
  ) {
    return false;
  }

  return true;
}

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
