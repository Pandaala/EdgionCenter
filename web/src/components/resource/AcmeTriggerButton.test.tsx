import { fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, expect, it, vi } from 'vitest'
import { message } from 'antd'
import { resourceApi } from '@/api/resources'
import AcmeTriggerButton from './AcmeTriggerButton'

const target = vi.hoisted(() => ({ controllerId: 'east/controller' }))
vi.mock('@/hooks/useControllerMutationTarget', () => ({ useControllerMutationTarget: () => ({ ...target }) }))
vi.mock('./PermissionAwareButton', () => ({ default: (props: any) =>
  <button data-testid={props['data-testid']} data-operation={props.operation} disabled={props.disabled} onClick={props.onClick}>{props.children}</button>,
}))
afterEach(() => { cleanup(); vi.restoreAllMocks(); target.controllerId = 'east/controller' })
const resource = { apiVersion: 'edgion.io/v1', kind: 'EdgionAcme', metadata: { name: 'cert', namespace: 'app' }, spec: {} }

it('gates by the dedicated operation, prevents duplicate dispatch and retains the captured target', async () => {
  let finish!: (value: 'queued') => void
  const trigger = vi.spyOn(resourceApi, 'triggerAcme').mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const toast = vi.spyOn(message, 'success').mockImplementation(() => (() => {}) as any)
  const client = new QueryClient()
  render(<QueryClientProvider client={client}><AcmeTriggerButton resource={resource} /></QueryClientProvider>)
  const button = screen.getByTestId('acme-trigger')
  expect(button).toHaveAttribute('data-operation', 'acme.trigger')
  fireEvent.click(button)
  fireEvent.click(button)
  expect(button).toBeDisabled()
  target.controllerId = 'west/controller'
  finish('queued')
  await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.stringContaining('not yet confirmed')))
  expect(trigger).toHaveBeenCalledTimes(1)
  expect(trigger).toHaveBeenCalledWith({ controllerId: 'east/controller' }, 'app', 'cert')
  client.clear()
})

it('reports ambiguous delivery without automatic retry or a success toast', async () => {
  const trigger = vi.spyOn(resourceApi, 'triggerAcme').mockResolvedValue('unknown')
  const success = vi.spyOn(message, 'success')
  const warning = vi.spyOn(message, 'warning').mockImplementation(() => (() => {}) as any)
  const client = new QueryClient()
  render(<QueryClientProvider client={client}><AcmeTriggerButton resource={resource} /></QueryClientProvider>)
  fireEvent.click(screen.getByTestId('acme-trigger'))
  await waitFor(() => expect(warning).toHaveBeenCalledWith(expect.stringContaining('unknown')))
  expect(success).not.toHaveBeenCalled()
  expect(trigger).toHaveBeenCalledTimes(1)
  client.clear()
})
