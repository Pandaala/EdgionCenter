import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { I18nProvider } from '@/i18n'
import CloudFrontPage, { freshOriginUpdate } from './CloudFrontPage'
import { cloudfrontMutationResult } from '@/api/cloudfront'

const state = vi.hoisted(() => ({ grants: new Set<string>(), listAccounts: vi.fn(), list: vi.fn(), observe: vi.fn(), create: vi.fn(), updateOrigin: vi.fn(), enable: vi.fn(), disable: vi.fn(), remove: vi.fn() }))

vi.mock('@/utils/permissions', () => ({ useCan: (permission: string) => state.grants.has(permission) }))
vi.mock('@/api/cloud', () => ({ cloudApi: { listAccounts: () => state.listAccounts() } }))
vi.mock('@/api/cloudfront', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/api/cloudfront')>()
  return { ...original, cloudfrontApi: {
    list: (...args: unknown[]) => state.list(...args), observe: (...args: unknown[]) => state.observe(...args),
    create: (...args: unknown[]) => state.create(...args), updateOrigin: (...args: unknown[]) => state.updateOrigin(...args),
    enable: (...args: unknown[]) => state.enable(...args), disable: (...args: unknown[]) => state.disable(...args), delete: (...args: unknown[]) => state.remove(...args),
  } }
})

const distribution = { id: 'E2ABCDE12345', arn: 'arn:aws:cloudfront::123456789012:distribution/E2ABCDE12345', domainName: 'd123.cloudfront.net', status: 'InProgress', enabled: false, etag: 'opaque-etag', deployed: false, webAclId: 'arn:aws:wafv2:us-east-1:123456789012:global/webacl/example/abc', supportedOrigin: { domainName: 'origin.example.com', httpsPort: 8443 } }

function renderPage() {
  return render(<I18nProvider><QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><CloudFrontPage writeAvailable /></QueryClientProvider></I18nProvider>)
}

async function selectAccount() {
  fireEvent.mouseDown(screen.getByRole('combobox'))
  fireEvent.click(await screen.findByText('AWS Main (aws-main)'))
  await screen.findByText(distribution.id)
}

beforeEach(() => {
  state.grants = new Set(['provider-accounts:read', 'cloudfront:read', 'cloudfront:write', 'cloudfront:disable', 'cloudfront:delete'])
  for (const mock of [state.listAccounts, state.list, state.observe, state.create, state.updateOrigin, state.enable, state.disable, state.remove]) mock.mockReset()
  state.listAccounts.mockResolvedValue({ success: true, data: [{ accountId: 'aws-main', displayName: 'AWS Main', provider: 'aws' }] })
  state.list.mockResolvedValue({ success: true, data: [distribution] })
  state.observe.mockResolvedValue({ success: true, data: distribution })
  state.create.mockResolvedValue({ success: true, data: distribution })
  state.updateOrigin.mockResolvedValue({ success: true, data: distribution })
  state.enable.mockResolvedValue({ success: true, data: { ...distribution, enabled: true } })
  state.disable.mockResolvedValue({ success: true, data: distribution })
  state.remove.mockResolvedValue(undefined)
})

describe('CloudFront dashboard boundary', () => {
  it('fails closed without CloudFront read permission', () => {
    state.grants.delete('cloudfront:read')
    renderPage()
    expect(screen.getByText('You do not have permission to read CloudFront Distribution inventory.')).toBeInTheDocument()
    expect(state.list).not.toHaveBeenCalled()
  })

  it('creates the bounded fixed-origin shape and keeps provider acceptance separate from deployment', async () => {
    renderPage()
    await selectAccount()
    expect(screen.getAllByText('Not deployed').length).toBeGreaterThan(0)
    fireEvent.click(screen.getByTestId('cloudfront-create'))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Caller Reference'), { target: { value: 'release-2026-07-23' } })
    fireEvent.change(within(dialog).getByLabelText('Origin Domain Name'), { target: { value: 'origin.example.com' } })
    fireEvent.change(within(dialog).getByLabelText('Origin HTTPS Port'), { target: { value: '8443' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }))
    await waitFor(() => expect(state.create).toHaveBeenCalledWith('aws-main', { callerReference: 'release-2026-07-23', originDomainName: 'origin.example.com', originHttpsPort: 8443 }))
    expect(await screen.findByText(/CloudFront accepted the request/)).toBeInTheDocument()
  })

  it('reads a fresh observation before updating the one bounded origin', async () => {
    await freshOriginUpdate('aws-main', distribution, { originDomainName: 'replacement.example.com', originHttpsPort: 443 })
    expect(state.observe).toHaveBeenCalledWith('aws-main', distribution.id)
    expect(state.updateOrigin).toHaveBeenCalledWith('aws-main', distribution.id, { originDomainName: 'replacement.example.com', originHttpsPort: 443 })
  })

  it('prefills the supported origin and exposes the planned AWS WAF navigation target', async () => {
    renderPage()
    await selectAccount()
    fireEvent.click(screen.getByRole('button', { name: 'Update Origin' }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByDisplayValue('origin.example.com')).toBeInTheDocument()
    expect(within(dialog).getByDisplayValue('8443')).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    fireEvent.click(screen.getByRole('button', { name: 'View' }))
    const waf = await screen.findByRole('link', { name: 'AWS WAF page coming soon' })
    expect(waf).toHaveAttribute('href', '/cloud/aws/waf')
  })

  it('renders conflict and unknown outcome separately, and locks further mutations until refresh after ambiguity', async () => {
    expect(cloudfrontMutationResult({ response: { status: 409, data: { error: 'conflict' } } })).toBe('conflicted')
    state.create.mockRejectedValueOnce({ response: { status: 409, data: { error: 'unknown_outcome' } } })
    renderPage()
    await selectAccount()
    fireEvent.click(screen.getByTestId('cloudfront-create'))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Caller Reference'), { target: { value: 'unknown-1' } })
    fireEvent.change(within(dialog).getByLabelText('Origin Domain Name'), { target: { value: 'origin.example.com' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }))
    expect(await screen.findByText(/Refresh is required before another mutation/)).toBeInTheDocument()
    expect(screen.getByTestId('cloudfront-create')).toBeDisabled()
  })

  it('requires the exact disabled Distribution ID before dispatching delete', async () => {
    renderPage()
    await selectAccount()
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    const dialog = await screen.findByRole('dialog')
    const confirm = within(dialog).getByRole('button', { name: 'Delete' })
    fireEvent.change(within(dialog).getByLabelText(`Type ${distribution.id} to continue.`), { target: { value: 'wrong-id' } })
    expect(state.remove).not.toHaveBeenCalled()
    fireEvent.change(within(dialog).getByLabelText(`Type ${distribution.id} to continue.`), { target: { value: distribution.id } })
    fireEvent.click(confirm)
    await waitFor(() => expect(state.remove).toHaveBeenCalledWith('aws-main', distribution.id))
  })
})
