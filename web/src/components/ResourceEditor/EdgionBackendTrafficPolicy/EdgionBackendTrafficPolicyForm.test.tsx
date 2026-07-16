import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import EdgionBackendTrafficPolicyForm from './EdgionBackendTrafficPolicyForm'
import type { EdgionBackendTrafficPolicy } from '@/types/edgion-backend-traffic-policy'

vi.mock('../common/MetadataSection', () => ({ default: () => null }))

describe('EdgionBackendTrafficPolicyForm', () => {
  it('narrowly edits one target while preserving siblings and unknown fields', () => {
    const onChange = vi.fn()
    const policy: EdgionBackendTrafficPolicy = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionBackendTrafficPolicy',
      metadata: { name: 'policy', namespace: 'prod' },
      spec: {
        targetRefs: [
          { group: '', kind: 'Service', name: 'service-a', futureRef: true },
          { group: '', kind: 'Service', name: 'service-b' },
        ],
        futureSpec: { preserve: true },
      },
    }

    render(<EdgionBackendTrafficPolicyForm data={policy} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue('service-a'), { target: { value: 'service-new' } })

    expect(onChange).toHaveBeenCalledWith({
      ...policy,
      spec: {
        ...policy.spec,
        targetRefs: [
          { group: '', kind: 'Service', name: 'service-new', futureRef: true },
          policy.spec.targetRefs[1],
        ],
      },
    })
  })

  it('removes only healthCheck.active and preserves unknown healthCheck siblings', () => {
    const onChange = vi.fn()
    const policy: EdgionBackendTrafficPolicy = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionBackendTrafficPolicy',
      metadata: { name: 'policy', namespace: 'prod' },
      spec: {
        targetRefs: [{ group: '', kind: 'Service', name: 'service-a' }],
        healthCheck: {
          active: {
            type: 'tcp', interval: '10s', timeout: '3s', healthyThreshold: 2,
            unhealthyThreshold: 3, expectedStatuses: [200],
          },
          futureProbePolicy: { preserve: true },
        },
      },
    }

    render(<EdgionBackendTrafficPolicyForm data={policy} onChange={onChange} />)
    fireEvent.click(screen.getAllByRole('switch')[1])

    expect(onChange).toHaveBeenCalledWith({
      ...policy,
      spec: {
        ...policy.spec,
        healthCheck: { futureProbePolicy: { preserve: true } },
      },
    })
  })

  it('reports a non-numeric expected status token instead of silently dropping it', () => {
    const onChange = vi.fn()
    const onDraftValidationChange = vi.fn()
    const policy: EdgionBackendTrafficPolicy = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionBackendTrafficPolicy',
      metadata: { name: 'policy', namespace: 'prod' },
      spec: {
        targetRefs: [{ group: '', kind: 'Service', name: 'service-a' }],
        healthCheck: {
          active: {
            type: 'http', path: '/', interval: '10s', timeout: '3s',
            healthyThreshold: 2, unhealthyThreshold: 3, expectedStatuses: [200],
          },
        },
      },
    }

    render(
      <EdgionBackendTrafficPolicyForm
        data={policy}
        onChange={onChange}
        onDraftValidationChange={onDraftValidationChange}
      />,
    )
    fireEvent.change(screen.getByDisplayValue('200'), { target: { value: '200, nope' } })

    expect(onChange).not.toHaveBeenCalled()
    expect(onDraftValidationChange).toHaveBeenLastCalledWith([
      'Expected HTTP statuses must be comma-separated integers.',
    ])
    expect(screen.getByDisplayValue('200, nope')).toBeInTheDocument()
  })

  it('discards an invalid HTTP status draft when switching HTTP to TCP and back', async () => {
    const onChange = vi.fn()
    const onDraftValidationChange = vi.fn()
    const policy: EdgionBackendTrafficPolicy = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionBackendTrafficPolicy',
      metadata: { name: 'policy', namespace: 'prod' },
      spec: {
        targetRefs: [{ group: '', kind: 'Service', name: 'service-a' }],
        healthCheck: {
          active: {
            type: 'http', path: '/', interval: '10s', timeout: '3s',
            healthyThreshold: 2, unhealthyThreshold: 3, expectedStatuses: [200],
          },
        },
      },
    }
    const { rerender } = render(
      <EdgionBackendTrafficPolicyForm
        data={policy}
        onChange={onChange}
        onDraftValidationChange={onDraftValidationChange}
      />,
    )
    fireEvent.change(screen.getByDisplayValue('200'), { target: { value: '200, nope' } })
    expect(screen.getByDisplayValue('200, nope')).toBeInTheDocument()

    const tcpPolicy = structuredClone(policy)
    tcpPolicy.spec.healthCheck!.active!.type = 'tcp'
    rerender(
      <EdgionBackendTrafficPolicyForm
        data={tcpPolicy}
        onChange={onChange}
        onDraftValidationChange={onDraftValidationChange}
      />,
    )
    await waitFor(() => expect(screen.queryByDisplayValue('200, nope')).not.toBeInTheDocument())

    rerender(
      <EdgionBackendTrafficPolicyForm
        data={policy}
        onChange={onChange}
        onDraftValidationChange={onDraftValidationChange}
      />,
    )
    expect(await screen.findByDisplayValue('200')).toBeInTheDocument()
    expect(onDraftValidationChange).toHaveBeenLastCalledWith([])
  })
})
