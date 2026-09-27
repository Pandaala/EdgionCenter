import { beforeEach, describe, expect, it, vi } from 'vitest'
import { AxiosError, AxiosHeaders } from 'axios'
import type { ReactElement } from 'react'
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import type { ProviderAccount } from '@/api/cloud'
import { renderWithQueryClient } from '@/test/render'
import ProviderAccountsPage from './ProviderAccountsPage'
import CloudflareDnsPage from './CloudflareDnsPage'

const state = vi.hoisted(() => ({
  grants: new Set<string>(),
  listAccounts: vi.fn(),
  createAccount: vi.fn(),
  getAccount: vi.fn(),
  replaceAccount: vi.fn(),
  getCapabilities: vi.fn(),
  inspectCredentials: vi.fn(),
  listZones: vi.fn(),
  listRecords: vi.fn(),
  putRecord: vi.fn(),
  deleteRecord: vi.fn(),
  createZone: vi.fn(),
  deleteZone: vi.fn(),
}))

vi.mock('@/utils/permissions', () => ({ useCan: (permission: string) => state.grants.has(permission) }))
vi.mock('@/api/cloud', () => ({
  cloudApi: {
    listAccounts: () => state.listAccounts(),
    createAccount: (...args: unknown[]) => state.createAccount(...args),
    getAccount: (...args: unknown[]) => state.getAccount(...args),
    replaceAccount: (...args: unknown[]) => state.replaceAccount(...args),
    getCapabilities: (...args: unknown[]) => state.getCapabilities(...args),
    inspectCredentials: (...args: unknown[]) => state.inspectCredentials(...args),
  },
}))
vi.mock('@/api/cloudflareDns', () => ({
  cloudflareDnsApi: {
    listZones: (...args: unknown[]) => state.listZones(...args),
    listRecords: (...args: unknown[]) => state.listRecords(...args),
    putRecord: (...args: unknown[]) => state.putRecord(...args),
    deleteRecord: (...args: unknown[]) => state.deleteRecord(...args),
    createZone: (...args: unknown[]) => state.createZone(...args),
    deleteZone: (...args: unknown[]) => state.deleteZone(...args),
  },
}))

const ACCOUNT: ProviderAccount = {
  accountId: 'cf-main', displayName: 'CF Main', owner: 'platform', labels: {}, generation: 2,
  managementPolicy: 'observe_only', deletionPolicy: 'retain', provider: 'cloudflare',
  scope: { provider: 'cloudflare', accountId: '0123456789abcdef0123456789abcdef' },
  credentialSource: { type: 'static_secret', credentialRef: 'mounted/cf-token' },
}

function renderPage(page: ReactElement) {
  return renderWithQueryClient(<I18nProvider>{page}</I18nProvider>)
}

beforeEach(() => {
  state.grants = new Set(['provider-accounts:read', 'provider-accounts:write', 'provider-credentials:use', 'provider-capabilities:read', 'provider-credentials:inspect', 'cloudflare-dns:read', 'cloudflare-dns:write'])
  for (const mock of [state.listAccounts, state.createAccount, state.getAccount, state.replaceAccount, state.getCapabilities, state.inspectCredentials, state.listZones, state.listRecords, state.putRecord, state.deleteRecord, state.createZone, state.deleteZone]) mock.mockReset()
  state.listAccounts.mockResolvedValue({ success: true, data: [ACCOUNT], count: 1 })
  state.createAccount.mockResolvedValue({ body: { success: true, data: ACCOUNT }, etag: '"1"' })
  state.getAccount.mockResolvedValue({ body: { success: true, data: ACCOUNT }, etag: '"2"' })
  state.getCapabilities.mockResolvedValue({ success: true, data: { providerAccountId: 'cf-main', provider: 'cloudflare', currentProviderAccountGeneration: 2, scope: { type: 'account' }, snapshotState: 'observed', snapshot: { observedAccountGeneration: 1, accountGenerationMatches: false, authorityState: 'stale', credentialAuthorityState: 'unknown', state: 'complete', discoveredAtUnixMs: 1, observations: [], issues: [] } } })
  state.inspectCredentials.mockResolvedValue({ success: true, data: { state: 'valid' } })
  state.listZones.mockResolvedValue({ success: true, data: { items: [{ providerAccountId: 'cf-main', zoneId: '0123456789abcdef0123456789abcdef', name: 'example.com.', kind: 'full', status: 'active', visibility: 'public', nameservers: ['a.ns.cloudflare.com.'], revision: 'zone-r1' }] } })
  state.listRecords.mockResolvedValue({ success: true, data: { items: [
    { providerAccountId: 'cf-main', zoneId: '0123456789abcdef0123456789abcdef', zoneApex: 'example.com.', zoneVisibility: 'public', owner: 'www.example.com.', recordType: 'A', ttl: { type: 'automatic' }, values: [{ type: 'A', address: '192.0.2.1' }], proxy: 'dns_only', cnameFlattening: 'provider_default', tags: [], control: { type: 'manual' }, providerObjectIds: ['0123456789abcdef0123456789abcdef'], revision: 'r1' },
    { providerAccountId: 'cf-main', zoneId: '0123456789abcdef0123456789abcdef', zoneApex: 'example.com.', zoneVisibility: 'public', owner: 'remote.example.com.', recordType: 'A', ttl: { type: 'automatic' }, values: [{ type: 'A', address: '192.0.2.2' }], proxy: 'proxied', cnameFlattening: 'provider_default', tags: [], control: { type: 'remote', callerAlias: 'abc_remote_alias' }, providerObjectIds: ['fedcba9876543210fedcba9876543210'], revision: 'r2' },
  ] } })
  state.putRecord.mockResolvedValue({ success: true })
})

describe('Provider accounts dashboard boundary', () => {
  it('submits a credential reference then destroys the input instead of rerendering it', async () => {
    renderPage(<ProviderAccountsPage />)
    await screen.findByText('cf-main')
    fireEvent.click(screen.getByTestId('cloud-account-create'))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Center Account ID'), { target: { value: 'cf-new' } })
    fireEvent.change(within(dialog).getByLabelText('Display Name'), { target: { value: 'CF New' } })
    fireEvent.change(within(dialog).getByLabelText('Provider Account ID'), { target: { value: '0123456789abcdef0123456789abcdef' } })
    fireEvent.change(within(dialog).getByLabelText('Credential Reference'), { target: { value: 'mounted/reference-only' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }))
    await waitFor(() => expect(state.createAccount).toHaveBeenCalledWith('cf-new', expect.objectContaining({ credentialSource: { type: 'static_secret', credentialRef: 'mounted/reference-only' } })))
    await waitFor(() => expect(screen.queryByDisplayValue('mounted/reference-only')).not.toBeInTheDocument())
  })

  it('binds edits to the opened snapshot and preserves labels on replacement', async () => {
    const account = { ...ACCOUNT, displayName: 'Current name', labels: { team: 'edge' } }
    state.getAccount.mockResolvedValueOnce({ body: { success: true, data: account }, etag: '"opened"' })
    state.getAccount.mockResolvedValue({ body: { success: true, data: { ...account, displayName: 'Concurrent name' } }, etag: '"newer"' })
    state.replaceAccount.mockResolvedValue({ body: { success: true, data: account }, etag: '"saved"' })
    renderPage(<ProviderAccountsPage />)
    await screen.findByText('cf-main')
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByLabelText('Display Name')).toHaveValue('Current name')
    fireEvent.change(within(dialog).getByLabelText('Display Name'), { target: { value: 'Edited name' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(state.replaceAccount).toHaveBeenCalledWith('cf-main', expect.objectContaining({ displayName: 'Edited name', labels: { team: 'edge' } }), '"opened"'))
    expect(state.getAccount).toHaveBeenCalledTimes(1)
  })

  it('keeps a conflicted edit draft and never refreshes its revision or retries', async () => {
    state.replaceAccount.mockRejectedValue({ response: { status: 412 } })
    renderPage(<ProviderAccountsPage />)
    await screen.findByText('cf-main')
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Display Name'), { target: { value: 'Unsaved draft' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(state.replaceAccount).toHaveBeenCalledOnce())
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Save' }).querySelector('.ant-btn-loading-icon')).toBeNull())
    expect(within(dialog).getByLabelText('Display Name')).toHaveValue('Unsaved draft')
    expect(state.getAccount).toHaveBeenCalledTimes(1)
    expect(state.replaceAccount.mock.calls[0][2]).toBe('"2"')
  })

  it('shows a failed edit load when the server omits the revision and sends no mutation', async () => {
    state.getAccount.mockResolvedValue({ body: { success: true, data: ACCOUNT } })
    renderPage(<ProviderAccountsPage />)
    await screen.findByText('cf-main')
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
    expect(await screen.findByText('Operation failed')).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(state.replaceAccount).not.toHaveBeenCalled()
  })

  it('shows stale capability evidence', async () => {
    renderPage(<ProviderAccountsPage />)
    await screen.findByText('cf-main')
    fireEvent.click(screen.getByRole('button', { name: 'Capability Evidence' }))
    expect(await screen.findByText(/older account generation/)).toBeInTheDocument()
    expect(state.getCapabilities).toHaveBeenCalledWith('cf-main')
  })

  it('fails closed when capability evidence permission is denied', async () => {
    state.grants.delete('provider-capabilities:read')
    renderPage(<ProviderAccountsPage />)
    await screen.findByText('cf-main')
    fireEvent.click(screen.getByRole('button', { name: 'Capability Evidence' }))
    expect(await screen.findByText('You do not have permission to read provider capability evidence.')).toBeInTheDocument()
    expect(state.getCapabilities).not.toHaveBeenCalled()
  })
})

describe('Cloudflare DNS dashboard boundary', () => {
  it.each(['accounts', 'zones', 'records'] as const)('shows sanitized %s read failures and recovers on refresh', async (boundary) => {
    const failed = boundary === 'accounts' ? state.listAccounts : boundary === 'zones' ? state.listZones : state.listRecords
    failed.mockRejectedValue(new Error('private-provider-error'))
    renderPage(<CloudflareDnsPage />)
    if (boundary !== 'accounts') {
      fireEvent.mouseDown(screen.getByRole('combobox'))
      fireEvent.click(await screen.findByText('CF Main (cf-main)'))
      if (boundary === 'records') fireEvent.click(await screen.findByTestId('cloudflare-zone-open'))
    }
    expect(await screen.findByText(/Could not load provider accounts or DNS inventory/)).toBeInTheDocument()
    expect(screen.queryByText('private-provider-error')).not.toBeInTheDocument()
    expect(screen.queryByText('No Zones found')).not.toBeInTheDocument()
    failed.mockResolvedValue(boundary === 'accounts' ? { success: true, data: [ACCOUNT] } : { success: true, data: { items: [] } })
    fireEvent.click(screen.getByRole('button', { name: /Refresh/ }))
    await waitFor(() => expect(screen.queryByText(/Could not load provider accounts or DNS inventory/)).not.toBeInTheDocument())
    expect(state.putRecord).not.toHaveBeenCalled()
    expect(state.deleteRecord).not.toHaveBeenCalled()
  })
  async function selectAccountAndOpenZone() {
    fireEvent.mouseDown(screen.getByRole('combobox'))
    fireEvent.click(await screen.findByText('CF Main (cf-main)'))
    fireEvent.click(await screen.findByTestId('cloudflare-zone-open'))
    await screen.findByText('Remote: abc_remote_alias')
  }

  it('renders the remote alias and keeps an applied result visible after a guarded edit', async () => {
    renderPage(<CloudflareDnsPage />)
    await selectAccountAndOpenZone()
    fireEvent.click(screen.getAllByRole('button', { name: 'Edit' })[0])
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Value'), { target: { value: '192.0.2.9' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(state.putRecord).toHaveBeenCalledWith('cf-main', '0123456789abcdef0123456789abcdef', expect.anything(), expect.objectContaining({ guard: { type: 'match_revision', revision: 'r1' } })))
    expect(await screen.findByText('The provider confirmed the change was applied.')).toBeInTheDocument()
  })

  it.each([
    ['conflict', { response: { status: 409, data: { error: 'conflict' } } }, 'The record changed before this request.'],
    ['unknown outcome', { response: { status: 503, data: { error: 'unknown_outcome' } } }, 'The provider outcome is unknown.'],
    ['lost browser response', new AxiosError('lost response', 'ERR_NETWORK', { method: 'put', headers: new AxiosHeaders() }, {}), 'The provider outcome is unknown.'],
  ])('renders %s results without retrying automatically', async (_name, error, expected) => {
    state.putRecord.mockRejectedValueOnce(error)
    renderPage(<CloudflareDnsPage />)
    await selectAccountAndOpenZone()
    fireEvent.click(screen.getAllByRole('button', { name: 'Edit' })[0])
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    expect(await screen.findByText(new RegExp(expected))).toBeInTheDocument()
    expect(state.putRecord).toHaveBeenCalledTimes(1)
  })

  it('renders an ambiguous result when creating a Zone has an unknown provider outcome', async () => {
    state.createZone.mockRejectedValueOnce({ response: { status: 503, data: { error: 'unknown_outcome' } } })
    renderPage(<CloudflareDnsPage />)
    fireEvent.mouseDown(screen.getByRole('combobox'))
    fireEvent.click(await screen.findByText('CF Main (cf-main)'))
    fireEvent.click(await screen.findByTestId('cloudflare-zone-create'))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Zone Name'), { target: { value: 'example.net' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }))
    expect(await screen.findByText(/provider outcome is unknown/)).toBeInTheDocument()
    expect(state.createZone).toHaveBeenCalledTimes(1)
  })

  it('does not query Cloudflare DNS when its read permission is absent', () => {
    state.grants.delete('cloudflare-dns:read')
    renderPage(<CloudflareDnsPage />)
    expect(screen.getByText('You do not have permission to read Cloudflare DNS inventory.')).toBeInTheDocument()
    expect(state.listZones).not.toHaveBeenCalled()
  })

  it('does not enumerate provider accounts when account-read permission is absent', () => {
    state.grants.delete('provider-accounts:read')
    renderPage(<CloudflareDnsPage />)
    expect(screen.getByText(/requires permission to read the provider accounts/)).toBeInTheDocument()
    expect(state.listAccounts).not.toHaveBeenCalled()
  })
})
