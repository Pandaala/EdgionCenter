import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { CredentialSourceConfig } from '@/types/link-sys'
import CredentialSourceFields from './CredentialSourceFields'

describe('CredentialSourceFields', () => {
  it('preserves credential rotation, TLS, and publication data on an endpoint edit', () => {
    const config: CredentialSourceConfig = {
      provider: { type: 'oauth2ClientCredentials', tokenEndpoint: 'https://issuer.example/token', clientAuthentication: { activeSecretRef: { name: 'active', namespace: 'edge', kind: 'Secret' }, previousSecretRef: { name: 'previous' } }, scopes: ['read'], tls: { enabled: false, clientCertificateRef: { name: 'client' } } },
      rotation: { interval: '30m' }, egress: { blockPrivate: false }, publication: { persist: false, memoryMaxKeys: 100 },
    }
    const onChange = vi.fn()
    render(<CredentialSourceFields config={config} onChange={onChange} readOnly={false} />)
    fireEvent.change(screen.getByTestId('linksys-credential-endpoint'), { target: { value: 'https://new.example/token' } })
    expect(onChange.mock.calls.at(-1)?.[0]).toEqual({ ...config, provider: { ...config.provider, tokenEndpoint: 'https://new.example/token' } })
    fireEvent.change(screen.getByDisplayValue('active'), { target: { value: 'rotated' } })
    expect(onChange.mock.calls.at(-1)?.[0].provider.clientAuthentication).toEqual({ ...config.provider.clientAuthentication, activeSecretRef: { name: 'rotated', namespace: 'edge', kind: 'Secret' } })
  })
})
