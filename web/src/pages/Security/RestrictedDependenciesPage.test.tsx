import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import RestrictedDependenciesPage from './RestrictedDependenciesPage'

const mocks = vi.hoisted(() => ({ allowed: false, listKeys: vi.fn() }))
vi.mock('@/hooks/useControllerAccess', () => ({ useControllerAccess: () => ({ authorizationPending:false, canResource: (_kind:string,verb:string) => mocks.allowed && verb === 'list-keys' }) }))
vi.mock('@/api/resources', () => ({ resourceApi: { listKeys: (...args:unknown[]) => mocks.listKeys(...args) } }))
vi.mock('@/components/resource/PermissionAwareButton', () => ({ default: (props:any) => <button disabled={!mocks.allowed} onClick={props.onClick}>{props.children}</button> }))
vi.mock('@/components/ResourceEditor/Secret/SecretEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/ConfigMap/ConfigMapEditor', () => ({ default: () => null }))

function renderPage() {
  const client = new QueryClient({ defaultOptions:{queries:{retry:false}} })
  return render(<MemoryRouter><QueryClientProvider client={client}><RestrictedDependenciesPage /></QueryClientProvider></MemoryRouter>)
}

describe('RestrictedDependenciesPage', () => {
  beforeEach(() => { mocks.allowed=false; mocks.listKeys.mockReset() })
  it('fails closed and never issues a metadata request without confirmed list-keys access', async () => {
    renderPage()
    expect(await screen.findByText('Metadata access denied for Secret')).toBeInTheDocument()
    expect(mocks.listKeys).not.toHaveBeenCalled()
    expect(screen.queryByRole('button',{name:/delete/i})).not.toBeInTheDocument()
    expect(screen.queryByRole('button',{name:/batch/i})).not.toBeInTheDocument()
  })
  it('renders only metadata returned by listKeys and never secret values', async () => {
    mocks.allowed=true
    mocks.listKeys.mockResolvedValue({success:true,count:1,data:[{apiVersion:'v1',kind:'Secret',metadata:{name:'db-password',namespace:'prod'}}]})
    renderPage()
    await waitFor(()=>expect(mocks.listKeys).toHaveBeenCalledWith('secret'))
    expect(await screen.findByText('db-password')).toBeInTheDocument()
    expect(screen.queryByText(/redacted/i)).not.toBeInTheDocument()
  })
})
