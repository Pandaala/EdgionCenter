import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { AuditListParams, AuditRecordDto } from '@/api/audit'
import AuditLogPage from './AuditLogPage'

// Mock the audit API module so the page renders against canned data.
const listMock = vi.fn()
vi.mock('@/api/audit', () => ({
  auditApi: {
    list: (params: AuditListParams) => listMock(params),
  },
}))

const ROWS: AuditRecordDto[] = [
  {
    ts: 1_700_000_300,
    actor: 'alice',
    provider: 'local',
    method: 'POST',
    path: '/api/v1/center/admin/controllers',
    targetController: 'c2',
    status: 500,
    sourceIp: '10.0.0.1',
    requestId: 'req-1',
    detail: null,
  },
  {
    ts: 1_700_000_200,
    actor: 'bob',
    provider: 'oidc',
    method: 'DELETE',
    path: '/api/v1/center/admin/controllers/c1',
    targetController: 'c1',
    status: 204,
    sourceIp: '10.0.0.2',
    requestId: 'req-2',
    detail: null,
  },
]

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <AuditLogPage />
    </QueryClientProvider>,
  )
}

describe('AuditLogPage', () => {
  beforeEach(() => {
    listMock.mockReset()
    listMock.mockResolvedValue({ success: true, data: ROWS, count: ROWS.length })
  })

  it('renders rows from the mocked audit API', async () => {
    renderPage()
    expect(await screen.findByText('alice')).toBeInTheDocument()
    expect(screen.getByText('bob')).toBeInTheDocument()
    // Target controller and status render too.
    expect(screen.getByText('c2')).toBeInTheDocument()
    expect(screen.getByText('500')).toBeInTheDocument()
  })

  it('refetches with the actor filter when Apply is clicked', async () => {
    renderPage()
    await screen.findByText('alice')
    expect(listMock).toHaveBeenCalledTimes(1)

    const actorInput = screen.getByPlaceholderText('Filter by actor')
    fireEvent.change(actorInput, { target: { value: 'alice' } })
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }))

    await waitFor(() => {
      expect(listMock).toHaveBeenCalledWith(expect.objectContaining({ actor: 'alice', offset: 0 }))
    })
  })
})
