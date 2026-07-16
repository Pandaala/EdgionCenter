/**
 * Gateway 工具函数
 */

import * as yaml from 'js-yaml'
import type { Gateway } from '@/types/gateway-api/gateway'
import { mutationDocumentToYaml } from './resource-document'

export const DEFAULT_GATEWAY_YAML = `apiVersion: gateway.networking.k8s.io/v1
kind: Gateway
metadata:
  name: my-gateway
  namespace: default
spec:
  gatewayClassName: edgion
  listeners:
    - name: http
      port: 80
      protocol: HTTP
    - name: https
      port: 443
      protocol: HTTPS
      tls:
        mode: Terminate
        certificateRefs:
          - name: my-cert
`

export function createEmptyGateway(): Gateway {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'Gateway',
    metadata: { name: '', namespace: 'default' },
    spec: {
      gatewayClassName: 'edgion',
      listeners: [
        { name: 'http', port: 80, protocol: 'HTTP' },
      ],
    },
  }
}

export function normalizeGateway(raw: unknown): Gateway {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('Gateway document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'Gateway') throw new Error('Expected a Gateway document')
  return structuredClone(document) as unknown as Gateway
}


export function gatewayToYaml(gw: Gateway): string {
  return yaml.dump(gw, { lineWidth: -1, noRefs: true })
}

export function gatewayToMutationYaml(gw: Gateway, mode: 'create' | 'update'): string {
  return mutationDocumentToYaml(gw, 'gateway', mode)
}

export function yamlToGateway(yamlStr: string): Gateway {
  const raw = yaml.load(yamlStr) as any
  return normalizeGateway(raw)
}

const ADDRESS_TYPE_PATTERN = /^(Hostname|IPAddress|NamedAddress|[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*\/[A-Za-z0-9/\-._~%!$&'()*+,;=:]+)$/
const PROTOCOL_PATTERN = /^(?:[a-zA-Z0-9](?:[-a-zA-Z0-9]*[a-zA-Z0-9])?|[a-z0-9](?:[-a-z0-9]*[a-z0-9])?(?:\.[a-z0-9](?:[-a-z0-9]*[a-z0-9])?)*\/[A-Za-z0-9]+)$/

function validateReference(ref: { name?: string; namespace?: string }, path: string, errors: string[]) {
  if (!ref.name?.trim()) errors.push(`${path}.name is required`)
}

function validateFrontendValidation(validation: any, path: string, errors: string[]) {
  if (!validation) return
  if (validation.mode && !['AllowValidOnly', 'AllowInsecureFallback'].includes(validation.mode)) errors.push(`${path}.mode is invalid`)
  const refs = validation.caCertificateRefs
  if (!Array.isArray(refs) || refs.length === 0) errors.push(`${path}.caCertificateRefs requires at least one reference`)
  else refs.forEach((ref: any, index: number) => validateReference(ref, `${path}.caCertificateRefs[${index}]`, errors))
}

export function validateGateway(resource: Gateway): string[] {
  const errors: string[] = []
  if (!resource.spec?.gatewayClassName?.trim()) errors.push('spec.gatewayClassName is required')
  if (!Array.isArray(resource.spec?.listeners) || resource.spec.listeners.length === 0) errors.push('spec.listeners requires at least one listener')
  const names = new Set<string>()
  ;(resource.spec?.listeners || []).forEach((listener, index) => {
    const path = `spec.listeners[${index}]`
    if (!listener.name?.trim()) errors.push(`${path}.name is required`)
    else if (names.has(listener.name)) errors.push(`${path}.name must be unique`)
    else names.add(listener.name)
    if (!Number.isInteger(listener.port) || listener.port < 1 || listener.port > 65535) errors.push(`${path}.port must be 1-65535`)
    if (!PROTOCOL_PATTERN.test(listener.protocol || '')) errors.push(`${path}.protocol is invalid`)
    if (['HTTP', 'TCP', 'UDP'].includes(listener.protocol) && listener.tls) errors.push(`${path}.tls must not be specified for ${listener.protocol}`)
    if (listener.protocol === 'HTTPS' && !listener.tls) errors.push(`${path}.tls is required for HTTPS`)
    if (listener.protocol === 'HTTPS' && listener.tls?.mode === 'Passthrough') errors.push(`${path}.tls.mode must be Terminate for HTTPS`)
    if (listener.protocol === 'TLS' && (!listener.tls || !listener.tls.mode)) errors.push(`${path}.tls.mode must be explicitly set for TLS`)
    if (['TCP', 'UDP'].includes(listener.protocol) && listener.hostname) errors.push(`${path}.hostname must not be specified for ${listener.protocol}`)
    if (listener.tls && (listener.tls.mode ?? 'Terminate') === 'Terminate') {
      if (!listener.tls.certificateRefs?.length && !Object.keys(listener.tls.options || {}).length) errors.push(`${path}.tls.certificateRefs or options is required for Terminate`)
      listener.tls.certificateRefs?.forEach((ref, refIndex) => validateReference(ref, `${path}.tls.certificateRefs[${refIndex}]`, errors))
    }
    validateFrontendValidation(listener.tls?.frontendValidation, `${path}.tls.frontendValidation`, errors)
    const namespaces = listener.allowedRoutes?.namespaces
    if (namespaces?.from === 'Selector') {
      const selector = namespaces.selector
      if (!selector || (!Object.keys(selector.matchLabels || {}).length && !(selector.matchExpressions || []).length)) errors.push(`${path}.allowedRoutes.namespaces.selector is required`)
      selector?.matchExpressions?.forEach((expression, expressionIndex) => {
        const expressionPath = `${path}.allowedRoutes.namespaces.selector.matchExpressions[${expressionIndex}]`
        if (!expression.key?.trim()) errors.push(`${expressionPath}.key is required`)
        if (['In', 'NotIn'].includes(expression.operator) && !expression.values?.length) errors.push(`${expressionPath}.values is required for ${expression.operator}`)
        if (['Exists', 'DoesNotExist'].includes(expression.operator) && expression.values?.length) errors.push(`${expressionPath}.values must be empty for ${expression.operator}`)
      })
    }
    listener.allowedRoutes?.kinds?.forEach((kind, kindIndex) => { if (!kind.kind?.trim()) errors.push(`${path}.allowedRoutes.kinds[${kindIndex}].kind is required`) })
  })
  resource.spec?.addresses?.forEach((address, index) => {
    if (address.type && !ADDRESS_TYPE_PATTERN.test(address.type)) errors.push(`spec.addresses[${index}].type is invalid`)
    if (!address.value?.trim()) errors.push(`spec.addresses[${index}].value is required`)
  })
  const globalTls = resource.spec?.tls
  if (globalTls?.backend?.clientCertificateRef) validateReference(globalTls.backend.clientCertificateRef, 'spec.tls.backend.clientCertificateRef', errors)
  validateFrontendValidation(globalTls?.frontend?.default?.validation, 'spec.tls.frontend.default.validation', errors)
  const ports = new Set<number>()
  globalTls?.frontend?.perPort?.forEach((entry, index) => {
    const path = `spec.tls.frontend.perPort[${index}]`
    if (!Number.isInteger(entry.port) || entry.port < 1 || entry.port > 65535) errors.push(`${path}.port must be 1-65535`)
    else if (ports.has(entry.port)) errors.push(`${path}.port must be unique`)
    else ports.add(entry.port)
    validateFrontendValidation(entry.tls?.validation, `${path}.tls.validation`, errors)
  })
  return errors
}
