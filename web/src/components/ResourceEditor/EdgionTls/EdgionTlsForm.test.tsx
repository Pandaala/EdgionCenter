import { useState } from 'react'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import EdgionTlsForm from './EdgionTlsForm'
import ParentRefsSection from '../common/ParentRefsSection'
import { normalizeEdgionTls, toMutationDocument } from '@/utils/edgiontls'
import type { EdgionTls } from '@/types/edgion-tls'

const fixture: EdgionTls = {
  apiVersion: 'edgion.io/v1', kind: 'EdgionTls',
  metadata: { name: 'certificate', namespace: 'edge', resourceVersion: '17' },
  spec: {
    hosts: ['api.example.com'], secretRef: { name: 'server-cert' },
    parentRefs: [
      { name: 'gateway-a', namespace: 'infra', sectionName: 'https-a', port: 443, group: 'gateway.networking.k8s.io', kind: 'Gateway', futureRef: { enabled: true } },
      { name: 'gateway-b', sectionName: 'https-b', port: 8443 },
    ],
    minTlsVersion: 'TLS1_3', ciphers: ['ECDHE-RSA-AES256-GCM-SHA384'], futureTls: false,
  },
}

function Harness({ initial = fixture, readOnly = false }: { initial?: EdgionTls; readOnly?: boolean }) {
  const [data, setData] = useState(() => normalizeEdgionTls(initial))
  return <>
    <EdgionTlsForm data={data} onChange={setData} readOnly={readOnly} isCreate={false} />
    <output data-testid="document">{JSON.stringify(toMutationDocument(data, 'update'))}</output>
  </>
}

function submitted(): EdgionTls {
  return JSON.parse(screen.getByTestId('document').textContent!) as EdgionTls
}

function parents() {
  return screen.getByText('Gateway 1', { exact: true }).closest('.ant-card')! as HTMLElement
}

describe('EdgionTls Gateway attachment form', () => {
  it('edits one reference while preserving all other attachment and TLS fields', () => {
    render(<Harness />)
    fireEvent.change(screen.getByDisplayValue('gateway-a'), { target: { value: 'gateway-renamed' } })
    expect(submitted()).toEqual({ ...fixture, spec: { ...fixture.spec, parentRefs: [
      { ...fixture.spec.parentRefs![0], name: 'gateway-renamed' }, fixture.spec.parentRefs![1],
    ] } })
    fireEvent.change(screen.getByDisplayValue('infra'), { target: { value: '' } })
    fireEvent.change(screen.getByDisplayValue('https-a'), { target: { value: '' } })
    expect(submitted().spec.parentRefs![0]).not.toHaveProperty('namespace')
    expect(submitted().spec.parentRefs![0]).not.toHaveProperty('sectionName')
    expect(submitted().spec.parentRefs![0].port).toBe(443)
  })

  it('removes the last optional attachment and creates a new Gateway reference in the resource namespace', () => {
    render(<Harness />)
    fireEvent.click(within(parents()).getByRole('button', { name: /Delete/ }))
    fireEvent.click(within(parents()).getByRole('button', { name: /Delete/ }))
    expect(submitted().spec).not.toHaveProperty('parentRefs')
    expect(submitted().spec.hosts).toEqual(fixture.spec.hosts)
    fireEvent.click(screen.getByRole('button', { name: /Add Gateway/ }))
    expect(submitted().spec.parentRefs).toEqual([{ group: 'gateway.networking.k8s.io', kind: 'Gateway', namespace: 'edge', name: '' }])
  })

  it('limits attachment additions to 32 and keeps read-only references immutable', () => {
    const initial = { ...fixture, spec: { ...fixture.spec, parentRefs: Array.from({ length: 32 }, (_, index) => ({ name: `gateway-${index}` })) } }
    const view = render(<Harness initial={initial} />)
    expect(screen.getByRole('button', { name: /Add Gateway/ })).toBeDisabled()
    view.unmount()
    render(<Harness readOnly />)
    expect(screen.getByDisplayValue('gateway-a')).toBeDisabled()
    expect(screen.queryByRole('button', { name: /Add Gateway/ })).not.toBeInTheDocument()
    expect(within(parents()).queryByRole('button', { name: /Delete/ })).not.toBeInTheDocument()
  })

  it('preserves the shared route editor default of retaining its last reference', () => {
    render(<ParentRefsSection value={[{ name: 'route-gateway' }]} />)
    expect(screen.queryByRole('button', { name: /Delete/ })).not.toBeInTheDocument()
  })
})
