/**
 * Kubernetes 和 Gateway API 通用类型定义
 */

// ============================================
// Kubernetes 通用类型
// ============================================

/**
 * Kubernetes ObjectMeta
 */
export interface K8sObjectMeta {
  name: string;
  namespace?: string;
  labels?: Record<string, string>;
  annotations?: Record<string, string>;
  creationTimestamp?: string;
  resourceVersion?: string;
  uid?: string;
}

/**
 * Kubernetes 资源基础接口
 */
export interface K8sResource<T = any> {
  apiVersion: string;
  kind: string;
  metadata: K8sObjectMeta;
  spec?: T;
  status?: any;
}

// ============================================
// Gateway API 通用别名
// ============================================

/**
 * Hostname - 主机名类型
 * 符合 RFC 1123，支持通配符前缀（如 *.example.com）
 */
export type Hostname = string;

/**
 * PreciseHostname - 精确主机名（不支持通配符）
 */
export type PreciseHostname = string;

/**
 * Duration - 时间间隔（如 "30s", "1m", "1h"）
 */
export type Duration = string;

// ============================================
// Gateway API 通用结构
// ============================================

/**
 * Kubernetes Condition
 */
export interface Condition {
  type: string;
  status: 'True' | 'False' | 'Unknown';
  observedGeneration?: number;
  lastTransitionTime: string;
  reason: string;
  message: string;
}

