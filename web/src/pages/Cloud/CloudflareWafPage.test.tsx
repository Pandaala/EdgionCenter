import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { I18nProvider } from '@/i18n'
import CloudflareWafPage from './CloudflareWafPage'

const state = vi.hoisted(() => ({
  grants: new Set<string>(), listAccounts: vi.fn(), listZones: vi.fn(), listRulesets: vi.fn(), create: vi.fn(), update: vi.fn(), order: vi.fn(), setManagedException: vi.fn(), securityWeaken: vi.fn(), delete: vi.fn(),
}))

vi.mock('@/utils/permissions', () => ({ useCan: (permission: string) => state.grants.has(permission) }))
vi.mock('@/api/cloud', () => ({ cloudApi: { listAccounts: () => state.listAccounts() } }))
vi.mock('@/api/cloudflareDns', () => ({ cloudflareDnsApi: { listZones: (...args: unknown[]) => state.listZones(...args) } }))
vi.mock('@/api/cloudflareWaf', () => ({ cloudflareWafApi: {
  listRulesets: (...args: unknown[]) => state.listRulesets(...args), create: (...args: unknown[]) => state.create(...args), update: (...args: unknown[]) => state.update(...args), order: (...args: unknown[]) => state.order(...args), setManagedException: (...args: unknown[]) => state.setManagedException(...args), securityWeaken: (...args: unknown[]) => state.securityWeaken(...args), delete: (...args: unknown[]) => state.delete(...args),
} }))

const zone = { providerAccountId: 'cf-main', zoneId: '0123456789abcdef0123456789abcdef', name: 'example.com.', kind: 'full', status: 'active', visibility: 'public', nameservers: [] }
const managed = { providerAccountId: 'cf-main', zoneId: zone.zoneId, phase: 'managed', availability: 'available', rulesetId: 'managed-entry', version: 'v1', rules: [
  { ruleId: 'center-rule', version: 'r1', action: 'execute', enabled: true, ownership: 'center_owned', position: 0, definition: { kind: 'managed', reference: 'managed-rule', description: 'Managed deployment', expression: 'http.request.uri.path eq "/"', managedRulesetId: 'cf-managed', overrides: [] } },
  { ruleId: 'managed-exception', version: 'r2', action: 'skip', enabled: true, ownership: 'center_owned', position: 1, definition: { kind: 'managed_exception', reference: 'managed-exception', description: 'Scoped exception', expression: 'http.request.uri.path eq "/health"', managedRulesetIds: ['cf-managed'], position: { type: 'index', index: 1 } } },
  { ruleId: 'provider-rule', version: 'r3', action: 'execute', enabled: true, ownership: 'observe_only', position: 2 },
] } as const
const custom = { providerAccountId: 'cf-main', zoneId: zone.zoneId, phase: 'custom', availability: 'available', rulesetId: 'custom-entry', version: 'v2', rules: [
  { ruleId: 'preview-rule', version: 'r3', action: 'log', enabled: true, ownership: 'observe_only', position: 0 },
] } as const
const rate = { providerAccountId: 'cf-main', zoneId: zone.zoneId, phase: 'rate_limit', availability: 'quota_limited', rules: [] } as const

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<I18nProvider><QueryClientProvider client={client}><CloudflareWafPage /></QueryClientProvider></I18nProvider>)
}

async function openZone() {
  fireEvent.mouseDown(screen.getByRole('combobox'))
  fireEvent.click(await screen.findByText('CF Main (cf-main)'))
  fireEvent.click(await screen.findByTestId('cloudflare-waf-zone-open'))
  await screen.findByText('managed-rule')
}

beforeEach(() => {
  state.grants = new Set(['provider-accounts:read', 'cloudflare-dns:read', 'cloudflare-waf:read', 'cloudflare-waf:write', 'cloudflare-waf:order', 'cloudflare-waf:exception', 'cloudflare-waf:security-weaken'])
  for (const item of [state.listAccounts, state.listZones, state.listRulesets, state.create, state.update, state.order, state.setManagedException, state.securityWeaken, state.delete]) item.mockReset()
  state.listAccounts.mockResolvedValue({ success: true, data: [{ accountId: 'cf-main', displayName: 'CF Main', provider: 'cloudflare' }] })
  state.listZones.mockResolvedValue({ success: true, data: { items: [zone] } })
  state.listRulesets.mockResolvedValue({ success: true, data: { rulesets: [managed, custom, rate] } })
  for (const item of [state.create, state.update, state.order, state.setManagedException, state.securityWeaken, state.delete]) item.mockResolvedValue({ success: true, data: {} })
})

describe('Cloudflare WAF dashboard boundary', () => {
  it('fails closed without each read prerequisite', () => {
    state.grants.delete('cloudflare-dns:read')
    renderPage()
    expect(screen.getByText(/requires Cloudflare DNS Zone inventory permission/)).toBeInTheDocument()
    expect(state.listAccounts).toHaveBeenCalledTimes(1)
    expect(state.listZones).not.toHaveBeenCalled()
    expect(state.listRulesets).not.toHaveBeenCalled()
  })

  it('shows opaque ordering blockers and only exposes bounded Center-owned editing', async () => {
    renderPage()
    await openZone()
    expect(screen.getByText(/provider-owned or opaque rule/)).toBeInTheDocument()
    expect(screen.getByText('provider-rule')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Edit' })).toHaveLength(1)
  })

  it('only exposes ordering and deletion for an existing managed exception', async () => {
    renderPage()
    await openZone()
    const exceptionRow = screen.getByRole('row', { name: /managed-exception/ })
    expect(within(exceptionRow).queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument()
    expect(within(exceptionRow).queryByRole('button', { name: 'Downgrade to log' })).not.toBeInTheDocument()
    expect(within(exceptionRow).getByRole('button', { name: 'Set Order' })).toBeInTheDocument()
    expect(within(exceptionRow).getByRole('button', { name: 'Delete' })).toBeInTheDocument()
  })

  it('submits a managed update with a ruleset version guard', async () => {
    renderPage()
    await openZone()
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Description'), { target: { value: 'Updated deployment' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(state.update).toHaveBeenCalledWith('cf-main', zone.zoneId, expect.objectContaining({ version: 'v1' }), expect.objectContaining({ ruleId: 'center-rule' }), expect.objectContaining({ description: 'Updated deployment' })))
  })

  it('uses the separate exception route and makes weakening explicit', async () => {
    renderPage()
    await openZone()
    fireEvent.click(screen.getByTestId('cloudflare-waf-exception'))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Reference'), { target: { value: 'exception-1' } })
    fireEvent.change(within(dialog).getByLabelText('Description'), { target: { value: 'Test exception' } })
    fireEvent.change(within(dialog).getByLabelText('Expression'), { target: { value: 'http.request.uri.path eq "/health"' } })
    fireEvent.change(within(dialog).getByLabelText('Managed Ruleset IDs (comma separated)'), { target: { value: 'cf-managed' } })
    fireEvent.change(within(dialog).getByLabelText('Security confirmation'), { target: { value: 'WEAKEN_CLOUDFLARE_WAF' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Confirm security weakening' }))
    await waitFor(() => expect(state.setManagedException).toHaveBeenCalledWith('cf-main', zone.zoneId, expect.objectContaining({ phase: 'managed' }), expect.objectContaining({ managedRulesetIds: ['cf-managed'] }), expect.anything()))
  })

  it('requires the weakening phrase before disabling a managed execute wrapper', async () => {
    renderPage()
    await openZone()
    fireEvent.click(screen.getByRole('button', { name: 'Downgrade to log' }))
    const dialog = await screen.findByRole('dialog')
    const confirm = within(dialog).getByRole('button', { name: 'Confirm security weakening' })
    expect(confirm).toBeDisabled()
    fireEvent.change(within(dialog).getByRole('textbox'), { target: { value: 'WEAKEN_CLOUDFLARE_WAF' } })
    fireEvent.click(confirm)
    await waitFor(() => expect(state.securityWeaken).toHaveBeenCalledWith('cf-main', zone.zoneId, expect.objectContaining({ phase: 'managed' }), expect.objectContaining({ ruleId: 'center-rule' }), expect.objectContaining({ managedRulesetId: 'cf-managed' })))
  })

  it('does not create a rate-limit rule when its entitlement is unavailable', async () => {
    renderPage()
    await openZone()
    fireEvent.click(screen.getByRole('tab', { name: 'Rate Limits' }))
    expect(await screen.findByText(/not currently available/)).toBeInTheDocument()
    expect(screen.queryByTestId('cloudflare-waf-create-rate_limit')).not.toBeInTheDocument()
  })

  it('renders the effective preview state for an observed log rule', async () => {
    renderPage()
    await openZone()
    fireEvent.click(screen.getByRole('tab', { name: 'Custom Rules' }))
    expect(await screen.findByText('Preview (log)')).toBeInTheDocument()
  })

  it('uses UTF-8 byte limits for the signed Center rule reference', async () => {
    renderPage()
    await openZone()
    fireEvent.click(screen.getByRole('tab', { name: 'Custom Rules' }))
    fireEvent.click(await screen.findByTestId('cloudflare-waf-create-custom'))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Reference'), { target: { value: '测'.repeat(31) } })
    fireEvent.change(within(dialog).getByLabelText('Description'), { target: { value: 'byte-safe' } })
    fireEvent.change(within(dialog).getByLabelText('Expression'), { target: { value: 'http.request.uri.path eq "/"' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }))
    expect(await within(dialog).findByText('Must not exceed 90 UTF-8 bytes.')).toBeInTheDocument()
    expect(state.create).not.toHaveBeenCalled()
  })

  it('allows a first custom rule when the entry point is absent', async () => {
    state.listRulesets.mockResolvedValueOnce({ success: true, data: { rulesets: [managed, { ...custom, availability: 'entry_point_absent', rulesetId: undefined, version: undefined }, rate] } })
    renderPage()
    await openZone()
    fireEvent.click(screen.getByRole('tab', { name: 'Custom Rules' }))
    expect(await screen.findByTestId('cloudflare-waf-create-custom')).toBeInTheDocument()
  })

  it('renders unknown outcomes without retrying the mutation', async () => {
    state.update.mockRejectedValueOnce({ response: { status: 503, data: { error: 'unknown_outcome' } } })
    renderPage()
    await openZone()
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    expect(await screen.findByText(/outcome is unknown/)).toBeInTheDocument()
    expect(state.update).toHaveBeenCalledTimes(1)
  })
})
