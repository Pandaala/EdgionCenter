import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, within } from '@testing-library/react'
import type { ComponentProps } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { I18nProvider } from '@/i18n'
import AwsWafPage, { ruleFromForm, type RuleValues } from './AwsWafPage'

const state = vi.hoisted(() => ({ grants: new Set<string>(), accounts: vi.fn(), acls: vi.fn(), ipSets: vi.fn(), catalog: vi.fn(), detail: vi.fn(), associations: vi.fn(), distributions: vi.fn() }))
vi.mock('@/utils/permissions', () => ({ useCan: (permission: string) => state.grants.has(permission) }))
vi.mock('@/api/cloud', () => ({ cloudApi: { listAccounts: () => state.accounts() } }))
vi.mock('@/api/awsWaf', () => ({ awsWafApi: { listWebAcls: (...args: unknown[]) => state.acls(...args), listIpSets: (...args: unknown[]) => state.ipSets(...args), listCatalog: (...args: unknown[]) => state.catalog(...args), getWebAcl: (...args: unknown[]) => state.detail(...args), associations: (...args: unknown[]) => state.associations(...args), capacity: vi.fn(), securityWeakenRule: vi.fn(), attachRegional: vi.fn(), detachRegional: vi.fn(), createWebAcl: vi.fn(), updateWebAcl: vi.fn(), updateRule: vi.fn(), createRule: vi.fn(), deleteRule: vi.fn(), managedException: vi.fn(), createIpSet: vi.fn(), updateIpSet: vi.fn(), deleteIpSet: vi.fn(), weakenWebAcl: vi.fn(), deleteWebAcl: vi.fn() }, isValidAwsRegion: (region: string) => /^us(?:-[a-z]+)+-\d+$/.test(region), awsWafMutationResult: () => 'rejected' }))
vi.mock('@/api/cloudfront', () => ({ cloudfrontApi: { list: (...args: unknown[]) => state.distributions(...args), setWebAcl: vi.fn() } }))

function renderPage(props: ComponentProps<typeof AwsWafPage> = { writeAvailable: true }) { return render(<I18nProvider><QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><AwsWafPage {...props} /></QueryClientProvider></I18nProvider>) }

beforeEach(() => {
  state.grants = new Set(['aws-waf:read', 'aws-waf:write', 'aws-waf:attach', 'aws-waf:detach', 'aws-waf:exception', 'aws-waf:security-weaken', 'provider-accounts:read'])
  for (const mock of [state.accounts, state.acls, state.ipSets, state.catalog, state.detail, state.associations, state.distributions]) mock.mockReset()
  state.accounts.mockResolvedValue({ data: [{ accountId: 'aws-main', displayName: 'AWS Main', provider: 'aws' }] })
  state.acls.mockResolvedValue({ data: [{ id: 'acl-1', name: 'edge', arn: 'arn:aws:wafv2:us-east-1:123:global/webacl/edge', scope: { type: 'cloudfront' }, capacity: 20, lockTokenPresent: true }] })
  state.ipSets.mockResolvedValue({ data: [] }); state.catalog.mockResolvedValue({ data: [] }); state.associations.mockResolvedValue({ data: [] }); state.distributions.mockResolvedValue({ data: [] })
  state.detail.mockResolvedValue({ data: { id: 'acl-1', name: 'edge', arn: 'arn:aws:wafv2:us-east-1:123:global/webacl/edge', scope: { type: 'cloudfront' }, defaultAction: 'block', visibility: { metricName: 'edge', cloudwatchMetricsEnabled: true, sampledRequestsEnabled: true }, capacity: 20, lockToken: 'opaque-lock', rules: [] } })
})

describe('AWS WAF dashboard boundary', () => {
  it('builds a managed-rule write with an explicit none override and preserves unedited overrides', () => {
    const values: RuleValues = { reference: 'managed-1', name: 'managed', priority: 1, kind: 'managed_rule_group', vendorName: 'AWS', managedName: 'AWSManagedRulesCommonRuleSet', excludedRules: 'NoUserAgent_HEADER', metricName: 'managed' }
    const existing = {
      name: 'managed', priority: 1, managedOverrideAction: 'none' as const, ownership: 'center_owned' as const, reference: 'managed-1', visibility: { metricName: 'managed', cloudwatchMetricsEnabled: true, sampledRequestsEnabled: true },
      statement: { kind: 'managed_rule_group' as const, vendorName: 'AWS', name: 'AWSManagedRulesCommonRuleSet', excludedRules: [], ruleActionOverrides: [{ name: 'SizeRestrictions_BODY', action: 'count' as const }] },
    }

    expect(ruleFromForm(values, 'opaque-lock', existing)).toEqual({
      reference: 'managed-1', lockToken: 'opaque-lock', name: 'managed', priority: 1,
      managedOverrideAction: 'none', visibility: existing.visibility,
      statement: { kind: 'managed_rule_group', vendorName: 'AWS', name: 'AWSManagedRulesCommonRuleSet', excludedRules: ['NoUserAgent_HEADER'], ruleActionOverrides: [{ name: 'SizeRestrictions_BODY', action: 'count' }] },
    })
  })

  it('builds non-managed references without an address-version field', () => {
    const values: RuleValues = { reference: 'ip-1', name: 'ip', priority: 1, action: 'block', kind: 'ip_set_reference', arn: 'arn:aws:wafv2:us-east-1:123:regional/ipset/edge/id', metricName: 'ip' }
    expect(ruleFromForm(values, 'opaque-lock')).toMatchObject({ action: 'block', statement: { kind: 'ip_set_reference', arn: values.arn } })
    expect(ruleFromForm(values, 'opaque-lock').statement).not.toHaveProperty('addressVersion')
  })

  it('keeps CloudFront and Regional scope selection explicit and exposes typed management entry points', async () => {
    renderPage()
    fireEvent.mouseDown(within(screen.getByTestId('aws-waf-account')).getByRole('combobox'))
    fireEvent.click(await screen.findByText('AWS Main (aws-main)'))
    expect(await screen.findByText('edge')).toBeInTheDocument()
    expect(screen.getAllByText('Create').length).toBeGreaterThan(0)
    fireEvent.mouseDown(within(screen.getByTestId('aws-waf-scope')).getByRole('combobox'))
    fireEvent.click(await screen.findByText('REGIONAL'))
    expect(screen.getByTestId('aws-waf-region')).toBeInTheDocument()
  })

  it('does not expose the CloudFront attach control until both WAF attach and CloudFront write are available', async () => {
    renderPage({ writeAvailable: true, attachAvailable: true, cloudfrontWriteAvailable: false })
    fireEvent.mouseDown(within(screen.getByTestId('aws-waf-account')).getByRole('combobox'))
    fireEvent.click(await screen.findByText('AWS Main (aws-main)'))
    fireEvent.click(await screen.findByRole('button', { name: 'View' }))
    expect(await screen.findByText('Web ACL Observation')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Attach to CloudFront Distribution' })).not.toBeInTheDocument()
  })
})
