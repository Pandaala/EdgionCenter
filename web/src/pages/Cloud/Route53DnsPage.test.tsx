import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { renderWithQueryClient } from '@/test/render'
import Route53DnsPage, { recordDesiredFromForm } from './Route53DnsPage'
import { route53MutationResult } from '@/api/route53Dns'

const state = vi.hoisted(() => ({ grants: new Set<string>(), listAccounts: vi.fn(), listZones: vi.fn(), listRecords: vi.fn(), putRecord: vi.fn(), deleteRecord: vi.fn(), createZone: vi.fn(), observeZoneLifecycle: vi.fn(), deleteZone: vi.fn() }))

vi.mock('@/utils/permissions', () => ({ useCan: (permission: string) => state.grants.has(permission) }))
vi.mock('@/api/cloud', () => ({ cloudApi: { listAccounts: () => state.listAccounts() } }))
vi.mock('@/api/route53Dns', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/api/route53Dns')>()
  return { ...original, route53DnsApi: {
    listZones: (...args: unknown[]) => state.listZones(...args), listRecords: (...args: unknown[]) => state.listRecords(...args),
    putRecord: (...args: unknown[]) => state.putRecord(...args), deleteRecord: (...args: unknown[]) => state.deleteRecord(...args),
    createZone: (...args: unknown[]) => state.createZone(...args), observeZoneLifecycle: (...args: unknown[]) => state.observeZoneLifecycle(...args), deleteZone: (...args: unknown[]) => state.deleteZone(...args),
  } }
})

const zone = { providerAccountId: 'aws-main', zoneId: 'Z0123456789ABCDEF', apex: 'example.com.', visibility: 'public' } as const
// These fixtures use the direct Rust serde response model, not the write DTO model.
const record = { providerAccountId: 'aws-main', zoneId: zone.zoneId, zoneApex: zone.apex, zoneVisibility: 'public', recordSet: { key: { owner: 'www.example.com.', recordType: 'A', routing: { type: 'route53', set_identifier: 'blue' } }, ttl: { type: 'inherited' }, values: [], extension: { provider: 'route53', alias_target: { targetZoneId: 'ZALIAS', target: 'dualstack.example.elb.amazonaws.com.', evaluateTargetHealth: true }, routing_policy: { type: 'weighted', weight: 10 }, health_check_id: 'hc-1' } }, control: 'external_or_manual', revision: 'r1' } as const
const txtRecord = { providerAccountId: 'aws-main', zoneId: zone.zoneId, zoneApex: zone.apex, zoneVisibility: 'public', recordSet: { key: { owner: '_check.example.com.', recordType: 'TXT', routing: { type: 'simple' } }, ttl: { type: 'seconds', seconds: 60 }, values: [{ type: 'TXT', value: [[104, 105]] }] }, control: 'external_or_manual', revision: 'r2' } as const

function renderPage() {
  return renderWithQueryClient(<I18nProvider><Route53DnsPage dnsWriteAvailable zoneLifecycleAvailable /></I18nProvider>)
}

beforeEach(() => {
  state.grants = new Set(['provider-accounts:read', 'route53-dns:read', 'route53-dns:write', 'route53-zones:write'])
  for (const mock of [state.listAccounts, state.listZones, state.listRecords, state.putRecord, state.deleteRecord, state.createZone, state.observeZoneLifecycle, state.deleteZone]) mock.mockReset()
  state.listAccounts.mockResolvedValue({ success: true, data: [{ accountId: 'aws-main', displayName: 'AWS Main', provider: 'aws' }] })
  state.listZones.mockResolvedValue({ success: true, data: { items: [zone] } })
  state.listRecords.mockResolvedValue({ success: true, data: { zone, items: [record, txtRecord] } })
  state.putRecord.mockResolvedValue({ success: true, data: { receipt: 'C1', providerApplication: 'PENDING', authoritativeConvergence: 'not_checked' } })
  state.observeZoneLifecycle.mockResolvedValue({ success: true, data: { zone, revision: 'z1', authoritativeNameservers: ['ns-1.awsdns.com.'], delegation: { state: 'delegated', expectedNameservers: ['ns-1.awsdns.com.'], parentNameservers: ['ns-1.awsdns.com.'] }, readiness: 'ready', dnssec: { state: 'disabled', dsRecords: [], externalAction: 'none' }, nonDefaultRecordCount: 0 } })
  state.deleteZone.mockResolvedValue({ success: true, data: { mutationId: 'M1', providerApplication: 'pending', authoritativeConvergence: 'not_checked' } })
})

describe('Route 53 DNS dashboard boundary', () => {
  it.each(['accounts', 'zones', 'records'] as const)('shows sanitized %s read failures and recovers on refresh', async (boundary) => {
    const failed = boundary === 'accounts' ? state.listAccounts : boundary === 'zones' ? state.listZones : state.listRecords
    failed.mockRejectedValue(new Error('private-provider-error'))
    renderPage()
    if (boundary !== 'accounts') {
      fireEvent.mouseDown(screen.getByRole('combobox'))
      fireEvent.click(await screen.findByText('AWS Main (aws-main)'))
      if (boundary === 'records') fireEvent.click(await screen.findByRole('button', { name: 'Records' }))
    }
    expect(await screen.findByText(/Could not load provider accounts or DNS inventory/)).toBeInTheDocument()
    expect(screen.queryByText('private-provider-error')).not.toBeInTheDocument()
    expect(screen.queryByText('No hosted zones found')).not.toBeInTheDocument()
    failed.mockResolvedValue(boundary === 'accounts' ? { success: true, data: [{ accountId: 'aws-main', displayName: 'AWS Main', provider: 'aws' }] } : { success: true, data: { items: [] } })
    fireEvent.click(screen.getByRole('button', { name: /Refresh/ }))
    await waitFor(() => expect(screen.queryByText(/Could not load provider accounts or DNS inventory/)).not.toBeInTheDocument())
    expect(state.putRecord).not.toHaveBeenCalled()
    expect(state.deleteRecord).not.toHaveBeenCalled()
  })
  it('serializes Alias, routing, and health-check fields using the native Alias shape', () => {
    expect(recordDesiredFromForm({ owner: 'api.example.com.', recordType: 'A', aliasEnabled: true, aliasTargetZoneId: 'ZALIAS', aliasTarget: 'dualstack.example.elb.amazonaws.com.', evaluateTargetHealth: true, routingKind: 'failover', failoverRole: 'primary', healthCheckId: 'hc-1' })).toEqual({
      ttl: { type: 'inherited' }, values: [], aliasTarget: { targetZoneId: 'ZALIAS', target: 'dualstack.example.elb.amazonaws.com.', evaluateTargetHealth: true }, routingPolicy: { type: 'failover', role: 'primary' }, healthCheckId: 'hc-1',
    })
  })

  it('classifies revision conflicts and unknown outcomes without retrying', () => {
    expect(route53MutationResult({ response: { status: 409, data: { error: 'conflict' } } })).toBe('conflicted')
    expect(route53MutationResult({ response: { status: 503, data: { error: 'unknown_outcome' } } })).toBe('ambiguous')
  })

  it('renders real Route 53 read fixtures and preserves Alias/routing/health identity on edit', async () => {
    renderPage()
    fireEvent.mouseDown(screen.getByRole('combobox'))
    fireEvent.click(await screen.findByText('AWS Main (aws-main)'))
    await screen.findByText('example.com.')
    fireEvent.click(screen.getByRole('button', { name: 'Inspect lifecycle' }))
    expect(await screen.findByText('disabled')).toBeInTheDocument()
    fireEvent.click(await screen.findByRole('button', { name: 'Records' }))
    expect(await screen.findByText('aGk')).toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Edit' })[0])
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByDisplayValue('ZALIAS')).toBeInTheDocument()
    expect(within(dialog).getByDisplayValue('dualstack.example.elb.amazonaws.com.')).toBeInTheDocument()
    expect(within(dialog).getByDisplayValue('blue')).toBeInTheDocument()
    expect(within(dialog).getByDisplayValue('hc-1')).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(state.putRecord).toHaveBeenCalledWith('aws-main', zone.zoneId, expect.objectContaining({ routing: { type: 'route53', set_identifier: 'blue' } }), expect.objectContaining({ aliasTarget: expect.objectContaining({ targetZoneId: 'ZALIAS' }), routingPolicy: { type: 'weighted', weight: 10 }, healthCheckId: 'hc-1' }), 'r1'))
  }, 15_000)

  it('requires the exact Zone Apex confirmation before it can dispatch deletion', async () => {
    renderPage()
    fireEvent.mouseDown(screen.getByRole('combobox'))
    fireEvent.click(await screen.findByText('AWS Main (aws-main)'))
    await screen.findByText('example.com.')
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    const dialog = await screen.findByRole('dialog')
    const confirm = within(dialog).getByRole('button', { name: 'Delete' })
    fireEvent.change(within(dialog).getByLabelText('Type example.com. to continue.'), { target: { value: 'wrong.example.' } })
    await waitFor(() => expect(state.deleteZone).not.toHaveBeenCalled())
    fireEvent.change(within(dialog).getByLabelText('Type example.com. to continue.'), { target: { value: 'example.com.' } })
    fireEvent.click(confirm)
    await waitFor(() => expect(state.deleteZone).toHaveBeenCalledTimes(1))
  })
})
