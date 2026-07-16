import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import StreamAnnotationsSection from './StreamAnnotationsSection'

describe('StreamAnnotationsSection runtime contract', () => {
  it('exposes StreamPlugins for UDP without TCP-only controls', () => {
    render(<StreamAnnotationsSection kind="UDPRoute" annotations={{}} onChange={vi.fn()} />)
    expect(screen.getByPlaceholderText('default/my-stream-plugins')).toBeInTheDocument()
    expect(screen.queryByText('TCP Keepalive Idle Time (seconds)')).not.toBeInTheDocument()
    expect(screen.queryByText('Proxy Protocol Version')).not.toBeInTheDocument()
  })

  it('exposes only keepalive controls for TCP', () => {
    render(<StreamAnnotationsSection kind="TCPRoute" annotations={{}} onChange={vi.fn()} />)
    expect(screen.getByText('TCP Keepalive Idle Time (seconds)')).toBeInTheDocument()
    expect(screen.queryByText('Proxy Protocol Version')).not.toBeInTheDocument()
    expect(screen.queryByText('Max Connection Retries')).not.toBeInTheDocument()
  })

  it('uses runtime-supported v2 and retry controls for TLS', () => {
    render(<StreamAnnotationsSection kind="TLSRoute" annotations={{ 'edgion.io/proxy-protocol': 'v2' }} onChange={vi.fn()} />)
    expect(screen.getByText('v2')).toBeInTheDocument()
    expect(screen.getByText('Max Connection Retries')).toBeInTheDocument()
    expect(screen.getByText('TCP Keepalive Probe Count')).toBeInTheDocument()
  })
})
