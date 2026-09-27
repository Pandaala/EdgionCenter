import { QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { renderWithQueryClient } from '@/test/render'
import RestrictedDependenciesPage from './RestrictedDependenciesPage'

const mocks = vi.hoisted(() => ({ allowed: false, listKeys: vi.fn() }))
vi.mock('@/hooks/useControllerAccess', () => ({ useControllerAccess: () => ({ authorizationPending:false, canResource: (_kind:string,verb:string) => mocks.allowed && verb === 'list-keys' }) }))
vi.mock('@/api/resources', () => ({ resourceApi: { listKeys: (...args:unknown[]) => mocks.listKeys(...args) } }))
vi.mock('@/components/resource/PermissionAwareButton', () => ({ default: (props:any) => <button disabled={!mocks.allowed} onClick={props.onClick}>{props.children}</button> }))
vi.mock('@/components/ResourceEditor/Secret/SecretEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/ConfigMap/ConfigMapEditor', () => ({ default: () => null }))

function renderPage() {
  return renderWithQueryClient(<MemoryRouter><RestrictedDependenciesPage /></MemoryRouter>)
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
    await waitFor(()=>expect(mocks.listKeys).toHaveBeenCalledWith('secret', { silent: true }))
    expect(await screen.findByText('db-password')).toBeInTheDocument()
    expect(screen.queryByText(/redacted/i)).not.toBeInTheDocument()
  })

  it.each(['Secret', 'ConfigMap'])('shows a sanitized %s read failure, retains cached rows and recovers', async (kind) => {
    mocks.allowed = true
    mocks.listKeys.mockResolvedValue({ success: true, data: [{ metadata: { name: 'cached-dependency', namespace: 'prod' } }] })
    renderPage()
    if (kind === 'ConfigMap') fireEvent.click(screen.getByTestId('configmap-tab'))
    expect(await screen.findByText('cached-dependency')).toBeInTheDocument()
    await waitFor(() => expect(mocks.listKeys).toHaveBeenCalledWith(kind.toLowerCase(), { silent: true }))
    mocks.listKeys.mockRejectedValue(new Error('credential-value-that-must-not-render'))
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    expect(await screen.findByText(`Unable to load ${kind} metadata`)).toBeInTheDocument()
    expect(screen.getByText('cached-dependency')).toBeInTheDocument()
    expect(screen.queryByText(/credential-value-that-must-not-render/)).not.toBeInTheDocument()
    mocks.listKeys.mockResolvedValue({ success: true, data: [{ metadata: { name: 'fresh-dependency', namespace: 'prod' } }] })
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    expect(await screen.findByText('fresh-dependency')).toBeInTheDocument()
    expect(screen.queryByText(`Unable to load ${kind} metadata`)).not.toBeInTheDocument()
    expect(screen.queryByText('cached-dependency')).not.toBeInTheDocument()
  })

  it('hides cached metadata when list permission is revoked', async () => {
    mocks.allowed = true
    mocks.listKeys.mockResolvedValue({ success: true, data: [{ metadata: { name: 'previously-visible', namespace: 'prod' } }] })
    const view = renderPage()
    expect(await screen.findByText('previously-visible')).toBeInTheDocument()
    const calls = mocks.listKeys.mock.calls.length
    mocks.allowed = false
    view.rerender(<QueryClientProvider client={view.queryClient}><MemoryRouter><RestrictedDependenciesPage /></MemoryRouter></QueryClientProvider>)
    expect(await screen.findByText('Metadata access denied for Secret')).toBeInTheDocument()
    expect(screen.queryByText('previously-visible')).not.toBeInTheDocument()
    expect(mocks.listKeys).toHaveBeenCalledTimes(calls)
  })

})
