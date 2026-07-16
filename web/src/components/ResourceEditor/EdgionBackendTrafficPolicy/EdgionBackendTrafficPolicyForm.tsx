import { useEffect, useState } from 'react'
import { Alert, Button, Card, Form, Input, InputNumber, Select, Space, Switch } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import MetadataSection from '../common/MetadataSection'
import type {
  ActiveHealthCheckConfig,
  EdgionBackendTrafficPolicy,
  EdgionBackendTrafficPolicySpec,
  LoadBalancerConfig,
  OutlierDetectionConfig,
  PolicyTargetRef,
  UpstreamAuthorityConfig,
} from '@/types/edgion-backend-traffic-policy'
import {
  createDefaultActiveHealthCheck,
  createDefaultLoadBalancer,
  createDefaultOutlierDetection,
  createDefaultUpstreamAuthority,
} from '@/utils/edgionbackendtrafficpolicy'
import { useT } from '@/i18n'

interface Props {
  data: EdgionBackendTrafficPolicy
  onChange: (data: EdgionBackendTrafficPolicy) => void
  readOnly?: boolean
  isCreate?: boolean
  onDraftValidationChange?: (errors: string[]) => void
}

function hasInvalidExpectedStatusTokens(source: string): boolean {
  const tokens = source.split(',').map((token) => token.trim())
  return tokens.length === 0 || tokens.some((token) => !/^\d+$/.test(token))
}

const EdgionBackendTrafficPolicyForm = ({
  data,
  onChange,
  readOnly = false,
  isCreate = true,
  onDraftValidationChange,
}: Props) => {
  const t = useT()
  const setSpec = (spec: EdgionBackendTrafficPolicySpec) => onChange({ ...data, spec })
  const setSection = <K extends keyof EdgionBackendTrafficPolicySpec>(key: K, value: EdgionBackendTrafficPolicySpec[K]) => {
    const next = { ...data.spec }
    if (value === undefined) delete next[key]
    else next[key] = value
    setSpec(next)
  }

  const patchTarget = (index: number, patch: Partial<PolicyTargetRef>) => {
    const refs = [...data.spec.targetRefs]
    refs[index] = { ...refs[index], ...patch }
    setSection('targetRefs', refs)
  }
  const patchLoadBalancer = (patch: Partial<LoadBalancerConfig>) =>
    setSection('loadBalancer', { ...data.spec.loadBalancer!, ...patch })
  const patchActive = (patch: Partial<ActiveHealthCheckConfig>) =>
    setSection('healthCheck', {
      ...data.spec.healthCheck,
      active: { ...data.spec.healthCheck!.active!, ...patch },
    })
  const patchOutlier = (patch: Partial<OutlierDetectionConfig>) =>
    setSection('outlierDetection', { ...data.spec.outlierDetection!, ...patch })
  const patchAuthority = (patch: Partial<UpstreamAuthorityConfig>) =>
    setSection('upstreamAuthority', { ...data.spec.upstreamAuthority!, ...patch })

  const lb = data.spec.loadBalancer
  const active = data.spec.healthCheck?.active
  const activeType = active?.type ?? 'http'
  const [expectedStatusesDraft, setExpectedStatusesDraft] = useState(
    () => (active?.expectedStatuses ?? []).join(', '),
  )
  const [expectedStatusesTouched, setExpectedStatusesTouched] = useState(false)
  const outlier = data.spec.outlierDetection
  const authority = data.spec.upstreamAuthority

  useEffect(() => {
    setExpectedStatusesDraft((active?.expectedStatuses ?? []).join(', '))
    setExpectedStatusesTouched(false)
  }, [active?.expectedStatuses])

  useEffect(() => {
    if (!active || activeType !== 'http') {
      setExpectedStatusesDraft((active?.expectedStatuses ?? []).join(', '))
      setExpectedStatusesTouched(false)
      onDraftValidationChange?.([])
    }
  }, [active, activeType, onDraftValidationChange])

  const toggleActiveHealthCheck = (checked: boolean) => {
    const healthCheck = { ...(data.spec.healthCheck ?? {}) }
    if (checked) healthCheck.active = createDefaultActiveHealthCheck()
    else delete healthCheck.active
    setSection('healthCheck', Object.keys(healthCheck).length > 0 ? healthCheck : undefined)
    setExpectedStatusesTouched(false)
    onDraftValidationChange?.([])
  }

  const updateExpectedStatuses = (source: string) => {
    setExpectedStatusesDraft(source)
    setExpectedStatusesTouched(true)
    const tokens = source.split(',').map((token) => token.trim())
    if (hasInvalidExpectedStatusTokens(source)) {
      onDraftValidationChange?.([t('validation.expectedStatusesTokens')])
      return
    }
    onDraftValidationChange?.([])
    patchActive({ expectedStatuses: tokens.map(Number) })
  }

  return (
    <Form layout="vertical" size="small" data-testid="ebtp-form">
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        <MetadataSection
          value={data.metadata}
          onChange={(metadata) => onChange({ ...data, metadata })}
          disabled={readOnly}
          isCreate={isCreate}
        />

        <Card title={t('section.targetRefs')} size="small">
          {data.spec.targetRefs.map((ref, index) => (
            <Card
              key={index}
              size="small"
              style={{ marginBottom: 8 }}
              extra={!readOnly ? (
                <Button
                  data-testid="edgionbackendtrafficpolicy-target-remove"
                  aria-label={t('btn.removeTargetRef')}
                  type="text"
                  danger
                  icon={<MinusCircleOutlined />}
                  disabled={data.spec.targetRefs.length === 1}
                  onClick={() => setSection('targetRefs', data.spec.targetRefs.filter((_, current) => current !== index))}
                />
              ) : null}
            >
              <Space wrap align="start">
                <Form.Item label={t('field.serviceName')} required>
                  <Input value={ref.name} disabled={readOnly} onChange={(event) => patchTarget(index, { name: event.target.value })} />
                </Form.Item>
                <Form.Item label={t('field.group')} tooltip={t('help.ebtpTargetGroup')}>
                  <Input value={ref.group} disabled={readOnly} onChange={(event) => patchTarget(index, { group: event.target.value })} />
                </Form.Item>
                <Form.Item label={t('field.kind')}>
                  <Select
                    value={ref.kind}
                    disabled={readOnly}
                    style={{ width: 150 }}
                    options={[{ value: 'Service', label: 'Service' }]}
                    onChange={(kind) => patchTarget(index, { kind })}
                  />
                </Form.Item>
              </Space>
            </Card>
          ))}
          {!readOnly && (
            <Button
              data-testid="edgionbackendtrafficpolicy-target-add"
              type="dashed"
              block
              icon={<PlusOutlined />}
              onClick={() => setSection('targetRefs', [...data.spec.targetRefs, { group: '', kind: 'Service', name: '' }])}
            >
              {t('btn.addTargetRef')}
            </Button>
          )}
        </Card>

        <Card
          title={t('section.loadBalancer')}
          size="small"
          extra={<Switch checked={Boolean(lb)} disabled={readOnly} onChange={(checked) => setSection('loadBalancer', checked ? createDefaultLoadBalancer() : undefined)} />}
        >
          {lb && (
            <>
              <Space wrap align="start">
                <Form.Item label={t('field.lbType')} required>
                  <Select
                    value={lb.type}
                    disabled={readOnly}
                    style={{ width: 180 }}
                    options={['RoundRobin', 'LeastConn', 'Ewma', 'ConsistentHash'].map((value) => ({ value, label: value }))}
                    onChange={(type: LoadBalancerConfig['type']) => {
                      const next: LoadBalancerConfig = { ...lb, type }
                      if (type === 'ConsistentHash' && !next.consistentHash) next.consistentHash = { hashOn: 'header', key: '' }
                      if (type !== 'ConsistentHash') delete next.consistentHash
                      setSection('loadBalancer', next)
                    }}
                  />
                </Form.Item>
                <Form.Item label={t('field.panicThreshold')} tooltip={t('help.panicThreshold')}>
                  <InputNumber min={0} max={100} value={lb.panicThreshold} disabled={readOnly} onChange={(value) => patchLoadBalancer({ panicThreshold: value ?? undefined })} />
                </Form.Item>
              </Space>
              {lb.type === 'ConsistentHash' && lb.consistentHash && (
                <Space wrap align="start">
                  <Form.Item label={t('field.hashOn')} required>
                    <Select
                      value={lb.consistentHash.hashOn}
                      disabled={readOnly}
                      style={{ width: 160 }}
                      options={['header', 'cookie', 'queryParam'].map((value) => ({ value, label: value }))}
                      onChange={(hashOn) => patchLoadBalancer({ consistentHash: { ...lb.consistentHash!, hashOn } })}
                    />
                  </Form.Item>
                  <Form.Item label={t('field.hashKey')} required>
                    <Input value={lb.consistentHash.key} disabled={readOnly} onChange={(event) => patchLoadBalancer({ consistentHash: { ...lb.consistentHash!, key: event.target.value } })} />
                  </Form.Item>
                </Space>
              )}
            </>
          )}
        </Card>

        <Card
          title={t('section.activeHealthCheck')}
          size="small"
          extra={<Switch checked={Boolean(active)} disabled={readOnly} onChange={toggleActiveHealthCheck} />}
        >
          {active && (
            <>
              <Space wrap align="start">
                <Form.Item label={t('field.healthCheckType')} required>
                  <Select
                    value={activeType}
                    disabled={readOnly}
                    style={{ width: 120 }}
                    options={['http', 'tcp', 'grpc'].map((value) => ({ value, label: value }))}
                    onChange={(type) => patchActive({ type })}
                  />
                </Form.Item>
                <Form.Item label={t('field.healthCheckPort')}>
                  <InputNumber min={0} max={65535} value={active.port} disabled={readOnly} onChange={(value) => patchActive({ port: value ?? undefined })} />
                </Form.Item>
                <Form.Item label={t('field.interval')} required>
                  <Input value={active.interval} disabled={readOnly} onChange={(event) => patchActive({ interval: event.target.value })} />
                </Form.Item>
                <Form.Item label={t('field.timeout')} required>
                  <Input value={active.timeout} disabled={readOnly} onChange={(event) => patchActive({ timeout: event.target.value })} />
                </Form.Item>
                <Form.Item label={t('field.healthyThreshold')} required>
                  <InputNumber min={1} value={active.healthyThreshold} disabled={readOnly} onChange={(value) => patchActive({ healthyThreshold: value ?? 0 })} />
                </Form.Item>
                <Form.Item label={t('field.unhealthyThreshold')} required>
                  <InputNumber min={1} value={active.unhealthyThreshold} disabled={readOnly} onChange={(value) => patchActive({ unhealthyThreshold: value ?? 0 })} />
                </Form.Item>
              </Space>
              {activeType === 'http' && (
                <Space wrap align="start">
                  <Form.Item label={t('field.healthCheckPath')} required>
                    <Input value={active.path} disabled={readOnly} onChange={(event) => patchActive({ path: event.target.value })} />
                  </Form.Item>
                  <Form.Item
                    label={t('field.expectedStatuses')}
                    required
                    tooltip={t('help.statusList')}
                    validateStatus={expectedStatusesTouched && hasInvalidExpectedStatusTokens(expectedStatusesDraft) ? 'error' : undefined}
                    help={expectedStatusesTouched && hasInvalidExpectedStatusTokens(expectedStatusesDraft)
                      ? t('validation.expectedStatusesTokens')
                      : undefined}
                  >
                    <Input
                      value={expectedStatusesDraft}
                      disabled={readOnly}
                      onChange={(event) => updateExpectedStatuses(event.target.value)}
                    />
                  </Form.Item>
                  <Form.Item label={t('field.healthCheckHost')}>
                    <Input value={active.host} disabled={readOnly} onChange={(event) => patchActive({ host: event.target.value || undefined })} />
                  </Form.Item>
                </Space>
              )}
              {activeType === 'grpc' && (
                <Form.Item label={t('field.grpcServiceName')}>
                  <Input value={active.grpcServiceName} disabled={readOnly} onChange={(event) => patchActive({ grpcServiceName: event.target.value || undefined })} />
                </Form.Item>
              )}
            </>
          )}
        </Card>

        <Card
          title={t('section.outlierDetection')}
          size="small"
          extra={<Switch checked={Boolean(outlier)} disabled={readOnly} onChange={(checked) => setSection('outlierDetection', checked ? createDefaultOutlierDetection() : undefined)} />}
        >
          {outlier && (
            <Space wrap align="start">
              <Form.Item label={t('field.consecutiveErrors')} required><InputNumber min={1} value={outlier.consecutiveErrors} disabled={readOnly} onChange={(value) => patchOutlier({ consecutiveErrors: value ?? 0 })} /></Form.Item>
              <Form.Item label={t('field.consecutiveGatewayErrors')} tooltip={t('help.inheritConsecutiveErrors')}><InputNumber min={1} value={outlier.consecutiveGatewayErrors} disabled={readOnly} onChange={(value) => patchOutlier({ consecutiveGatewayErrors: value ?? undefined })} /></Form.Item>
              <Form.Item label={t('field.consecutiveLocalOriginFailures')} tooltip={t('help.inheritGatewayErrors')}><InputNumber min={1} value={outlier.consecutiveLocalOriginFailures} disabled={readOnly} onChange={(value) => patchOutlier({ consecutiveLocalOriginFailures: value ?? undefined })} /></Form.Item>
              <Form.Item label={t('field.ejectionSeconds')} required><InputNumber min={1} value={outlier.ejectionSeconds} disabled={readOnly} onChange={(value) => patchOutlier({ ejectionSeconds: value ?? 0 })} /></Form.Item>
              <Form.Item label={t('field.maxEjectionSeconds')} tooltip={t('help.maxEjectionSeconds')}><InputNumber min={1} value={outlier.maxEjectionSeconds} disabled={readOnly} onChange={(value) => patchOutlier({ maxEjectionSeconds: value ?? undefined })} /></Form.Item>
              <Form.Item label={t('field.maxEjectionPercent')} required><InputNumber min={0} max={100} value={outlier.maxEjectionPercent} disabled={readOnly} onChange={(value) => patchOutlier({ maxEjectionPercent: value ?? 0 })} /></Form.Item>
            </Space>
          )}
        </Card>

        <Card
          title={t('section.upstreamAuthority')}
          size="small"
          extra={<Switch checked={Boolean(authority)} disabled={readOnly} onChange={(checked) => setSection('upstreamAuthority', checked ? createDefaultUpstreamAuthority() : undefined)} />}
        >
          {authority && (
            <Space direction="vertical" style={{ width: '100%' }}>
              <Alert type="warning" showIcon message={t('notice.upstreamAuthoritySafety')} />
              {active && !authority.healthCheckHost && <Alert type="error" showIcon message={t('notice.healthCheckHostRequired')} />}
              <Form.Item label={t('field.authorityPattern')} required><Input value={authority.pattern} disabled={readOnly} onChange={(event) => patchAuthority({ pattern: event.target.value })} /></Form.Item>
              <Form.Item label={t('field.authorityTemplate')} required tooltip={t('help.authorityTemplate')}><Input value={authority.template} disabled={readOnly} onChange={(event) => patchAuthority({ template: event.target.value })} /></Form.Item>
              <Form.Item label={t('field.authorityHealthCheckHost')} required={Boolean(active)}><Input value={authority.healthCheckHost} disabled={readOnly} onChange={(event) => patchAuthority({ healthCheckHost: event.target.value || undefined })} /></Form.Item>
            </Space>
          )}
        </Card>
      </Space>
    </Form>
  )
}

export default EdgionBackendTrafficPolicyForm
