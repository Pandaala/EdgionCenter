import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import * as yaml from 'js-yaml'
import { renderWithQueryClient } from '@/test/render'
import { createEmptyEdgionBackend } from '@/utils/edgionbackend'
import { message } from 'antd'
import EdgionBackendEditor from './EdgionBackendEditor'

const create = vi.fn()
const update = vi.fn()
vi.mock('@/api/resources', () => ({ resourceApi: {
  create: (...args: unknown[]) => create(...args), update: (...args: unknown[]) => update(...args),
} }))
vi.mock('@/components/YamlEditor', () => ({ default: ({ value, onChange }: { value: string; onChange: (value: string) => void }) => (
  <textarea aria-label="Backend YAML" value={value} onChange={(event) => onChange(event.target.value)} />
) }))
vi.mock('@/components/resource/PermissionAwareButton', () => ({ default: (props: any) => {
  const copy = { ...props }
  delete copy.resourceKind; delete copy.resourceVerb; delete copy.loading
  return <button {...copy} />
} }))

function fixture() {
  const value = createEmptyEdgionBackend()
  value.metadata = { name: 'provider', namespace: 'edge', resourceVersion: '42' }
  value.spec.ai.endpoint = 'https://old.example.test'
  value.spec.ai.credentialPool.credentials = [{ name: 'primary', secretRef: { name: 'provider-key' }, secret: '[redacted]' }]
  value.spec.ai.models = [{ name: 'model-one', aliases: ['default'], public: false }, { name: 'model-two' }]
  value.spec.futureField = { preserved: true }
  value.spec.currentStatus = { conditions: [] }
  return value
}

function renderEditor(mode: 'create' | 'edit') {
  renderWithQueryClient(<MemoryRouter initialEntries={['/controller/east~one']}><Routes>
    <Route path="/controller/:controllerId" element={<EdgionBackendEditor visible mode={mode} resource={mode === 'edit' ? fixture() : undefined} onClose={vi.fn()} />} />
  </Routes></MemoryRouter>)
}

describe('EdgionBackend editor', () => {
  beforeEach(() => { create.mockReset(); update.mockReset(); create.mockResolvedValue({ success: true }); update.mockResolvedValue({ success: true }) })

  it('submits a narrow form edit to the captured Controller with CAS and preserved siblings', async () => {
    renderEditor('edit')
    fireEvent.change(screen.getByDisplayValue('https://old.example.test'), { target: { value: 'https://new.example.test' } })
    fireEvent.click(screen.getByTestId('editor-yaml-tab'))
    await waitFor(() => expect((screen.getByLabelText('Backend YAML') as HTMLTextAreaElement).value).toContain('https://new.example.test'))
    fireEvent.click(screen.getByTestId('editor-form-tab'))
    fireEvent.click(screen.getByTestId('editor-submit'))
    await waitFor(() => expect(update).toHaveBeenCalledOnce())
    expect(update.mock.calls[0].slice(0, 4)).toEqual([{ controllerId: 'east/one' }, 'edgionbackend', 'edge', 'provider'])
    const payload = yaml.load(update.mock.calls[0][4]) as any
    expect(payload.metadata.resourceVersion).toBe('42')
    expect(payload.spec.ai.models).toEqual(fixture().spec.ai.models)
    expect(payload.spec.ai.endpoint).toBe('https://new.example.test')
    expect(payload.spec.futureField).toEqual({ preserved: true })
    expect(payload.spec.currentStatus).toBeUndefined()
    expect(payload.spec.ai.credentialPool.credentials[0].secret).toBeUndefined()
  })

  it('creates from YAML without replaying server metadata', async () => {
    renderEditor('create')
    fireEvent.click(screen.getByTestId('editor-yaml-tab'))
    fireEvent.change(await screen.findByLabelText('Backend YAML'), { target: { value: yaml.dump(fixture()) } })
    fireEvent.click(screen.getByTestId('editor-submit'))
    await waitFor(() => expect(create).toHaveBeenCalledOnce())
    expect(create.mock.calls[0].slice(0, 3)).toEqual([{ controllerId: 'east/one' }, 'edgionbackend', 'edge'])
    expect((yaml.load(create.mock.calls[0][3]) as any).metadata.resourceVersion).toBeUndefined()
  })

  it('rejects a YAML rename before dispatching an update', async () => {
    const error = vi.spyOn(message, 'error').mockImplementation(() => (() => {}) as ReturnType<typeof message.error>)
    try {
      renderEditor('edit')
      fireEvent.click(screen.getByTestId('editor-yaml-tab'))
      const renamed = fixture()
      renamed.metadata.name = 'different-provider'
      fireEvent.change(await screen.findByLabelText('Backend YAML'), { target: { value: yaml.dump(renamed) } })
      fireEvent.click(screen.getByTestId('editor-submit'))
      await waitFor(() => expect(error).toHaveBeenCalled())
      expect(update).not.toHaveBeenCalled()
    } finally { error.mockRestore() }
  })
})
