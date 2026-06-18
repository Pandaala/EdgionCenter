// API Response types
export interface ApiResponse<T> {
  success: boolean
  data?: T
  error?: string
}

export interface ListResponse<T> {
  success: boolean
  data?: T[]
  count: number
  continue_token?: string
  error?: string
}

// K8s Resource base interface
export interface K8sMetadata {
  name: string
  namespace?: string
  labels?: Record<string, string>
  annotations?: Record<string, string>
  resourceVersion?: string
  creationTimestamp?: string
}

export interface K8sResource {
  apiVersion: string
  kind: string
  metadata: K8sMetadata
  spec?: any
  status?: any
}

// Metadata-only resource key, returned by /api/v1/keys/{cluster|namespaced}/{kind}.
// Mirrors the backend ResourceKey / ResourceKeyMeta types. Same shape as
// K8sResource minus spec/status, so consumers cannot reach for fields that
// the metadata-only endpoints do not carry.
export interface ResourceKey {
  apiVersion: string
  kind: string
  metadata: K8sMetadata
}

// Resource kinds
export type ResourceKind =
  | 'httproute'
  | 'grpcroute'
  | 'tcproute'
  | 'udproute'
  | 'tlsroute'
  | 'service'
  | 'endpointslice'
  | 'edgiontls'
  | 'edgionplugins'
  | 'edgionconfigdata'
  | 'linksys'
  | 'secret'
  | 'gatewayclass'
  | 'edgiongatewayconfig'
  | 'gateway'
  | 'edgionstreamplugins'
  | 'referencegrant'
  | 'edgionacme'
  | 'backendtlspolicy'

export type ResourceScope = 'namespaced' | 'cluster'

