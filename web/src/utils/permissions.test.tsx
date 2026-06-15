import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { PermissionProvider, useCan } from './permissions'

// Mock the auth API so the provider resolves against canned permissions.
const meMock = vi.fn()
vi.mock('../api/auth', () => ({
  authApi: {
    me: () => meMock(),
  },
}))

function Probe({ permission }: { permission: string }) {
  const can = useCan(permission)
  return <span data-testid="result">{can ? 'yes' : 'no'}</span>
}

describe('useCan', () => {
  beforeEach(() => {
    meMock.mockReset()
  })

  it('returns true for a present key', async () => {
    meMock.mockResolvedValue({ success: true, data: { username: 'admin', permissions: ['controllers:read'] } })
    render(
      <PermissionProvider>
        <Probe permission="controllers:read" />
      </PermissionProvider>,
    )
    await waitFor(() => expect(screen.getByTestId('result').textContent).toBe('yes'))
  })

  it('returns false for an absent key', async () => {
    meMock.mockResolvedValue({ success: true, data: { username: 'admin', permissions: ['controllers:read'] } })
    render(
      <PermissionProvider>
        <Probe permission="users:manage" />
      </PermissionProvider>,
    )
    // Settle the async fetch, then assert the absent key is denied.
    await waitFor(() => expect(meMock).toHaveBeenCalled())
    expect(screen.getByTestId('result').textContent).toBe('no')
  })

  it('treats a missing permissions field as no permissions', async () => {
    meMock.mockResolvedValue({ success: true, data: { username: 'admin' } })
    render(
      <PermissionProvider>
        <Probe permission="controllers:read" />
      </PermissionProvider>,
    )
    await waitFor(() => expect(meMock).toHaveBeenCalled())
    expect(screen.getByTestId('result').textContent).toBe('no')
  })
})
