import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, screen, waitFor } from '@testing-library/react'
import { createTestQueryClient, renderWithQueryClient } from '@/test/render'
import LinkSysEditor from './LinkSysEditor'

const create = vi.fn()

vi.mock('@/api/resources', () => ({
  resourceApi: {
    create: (...args: unknown[]) => create(...args),
    update: vi.fn(),
  },
}))

vi.mock('@/components/YamlEditor', () => ({
  default: ({ value, onChange }: any) => <textarea aria-label="yaml-source" value={value} onChange={(e) => onChange(e.target.value)} />,
}))

describe('LinkSysEditor', () => {
  beforeEach(() => {
    create.mockReset()
    create.mockResolvedValue({ success: true })
  })

  it('creates the current wire shape and invalidates the resource list', async () => {
    const client = createTestQueryClient()
    const invalidate = vi.spyOn(client, 'invalidateQueries')

    renderWithQueryClient(<LinkSysEditor visible mode="create" onClose={vi.fn()} />, client)

    fireEvent.change(screen.getByPlaceholderText('example-route'), { target: { value: 'redis-test' } })
    const typeInput = screen.getAllByRole('combobox')[0]
    fireEvent.mouseDown(typeInput)
    fireEvent.click((await screen.findAllByText('HTTP DNS')).at(-1)!)
    const presetInput = screen.getAllByRole('combobox')[1]
    fireEvent.mouseDown(presetInput)
    fireEvent.click((await screen.findAllByText('Aliyun')).at(-1)!)
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))

    await waitFor(() => expect(create).toHaveBeenCalledOnce())
    expect(create.mock.calls[0][1]).toBe('linksys')
    expect(create.mock.calls[0][2]).toBe('default')
    expect(create.mock.calls[0][3]).toContain('config:')
    expect(create.mock.calls[0][3]).toContain('preset: aliyun')
    expect(create.mock.calls[0][3]).not.toContain('resolvedSecrets:')
    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ['resource-list', 'linksys'] })
    })
  })

  it('creates a YAML LinkSys through the same validated request boundary', async () => {
    renderWithQueryClient(<LinkSysEditor visible mode="create" onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('yaml-source'), { target: { value: 'apiVersion: edgion.io/v1\nkind: LinkSys\nmetadata: {name: kafka-test, namespace: prod}\nspec: {type: kafka, config: {brokers: [kafka:9092], channelSize: 100}}\n' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await waitFor(() => expect(create).toHaveBeenCalledWith(
      { controllerId: null },
      'linksys',
      'prod',
      expect.stringContaining('channelSize: 100'),
    ))
  })
})
