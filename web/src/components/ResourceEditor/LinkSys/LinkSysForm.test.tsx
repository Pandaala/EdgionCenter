import { useState } from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import LinkSysForm from './LinkSysForm'
import { createEmpty } from '@/utils/linksys'
import type { LinkSys } from '@/types/link-sys'

function Harness(){const [value,setValue]=useState<LinkSys>(createEmpty());return <LinkSysForm data={value} onChange={setValue}/>}
describe('LinkSysForm variant drafts',()=>{
  it('restores an edited variant after switching away and back',async()=>{
    render(<Harness/>)
    const dbItem=screen.getByText('Database Number').closest('.ant-form-item')!
    fireEvent.change(dbItem.querySelector('input')!,{target:{value:'3'}})
    const typeInput=screen.getAllByRole('combobox')[0]
    fireEvent.mouseDown(typeInput);fireEvent.click((await screen.findAllByText('Kafka')).at(-1)!)
    fireEvent.mouseDown(screen.getAllByRole('combobox')[0]);fireEvent.click((await screen.findAllByText('Redis')).at(-1)!)
    const restored=screen.getByText('Database Number').closest('.ant-form-item')!.querySelector('input')!
    expect(restored).toHaveValue('3')
  })

  it('does not expose removed webhook degradation controls', async () => {
    render(<Harness/>)
    fireEvent.mouseDown(screen.getAllByRole('combobox')[0])
    fireEvent.click((await screen.findAllByText('Webhook')).at(-1)!)
    expect(screen.queryByText('Allow degradation')).not.toBeInTheDocument()
    expect(screen.queryByText('Degradation template')).not.toBeInTheDocument()
  })
})


describe('OTLP form', () => {
  it('preserves TLS and Secret identity fields during narrow edits', () => {
    const resource = createEmpty()
    resource.spec = { type: 'otlp', config: { endpoint: 'https://collector.example:4317', timeoutMs: 5000, auth: { secretRef: { name: 'token', namespace: 'edge', kind: 'Secret', group: 'core' } }, tls: { enabled: false, verify: true } } }
    const onChange = vi.fn()
    render(<LinkSysForm data={resource} onChange={onChange} />)
    fireEvent.change(screen.getByTestId('linksys-otlp-endpoint'), { target: { value: 'https://new.example:4317' } })
    expect(onChange.mock.calls.at(-1)?.[0].spec.config).toEqual({ ...resource.spec.config, endpoint: 'https://new.example:4317' })
    fireEvent.change(screen.getByDisplayValue('token'), { target: { value: 'rotated-token' } })
    expect(onChange.mock.calls.at(-1)?.[0].spec.config.auth.secretRef).toEqual({ name: 'rotated-token', namespace: 'edge', kind: 'Secret', group: 'core' })
  })
})

describe('current Redis controls', () => {
  it('edits duration strings and removes conflicting fields when selecting Sentinel', async () => {
    const resource = createEmpty()
    resource.spec.config = { endpoints: ['redis://cache:6379'], db: 255, timeout: { connect: '5s', command: '30s' }, topology: { mode: 'cluster', cluster: { maxRedirects: 8 } } }
    const onChange = vi.fn()
    render(<LinkSysForm data={resource} onChange={onChange} />)
    fireEvent.change(screen.getByLabelText('Redis connect timeout'), { target: { value: '7s' } })
    expect(onChange.mock.calls.at(-1)?.[0].spec.config.timeout).toEqual({ connect: '7s', command: '30s' })
    expect(screen.queryByText('Read from replicas')).not.toBeInTheDocument()
    expect(screen.queryByText('Min idle')).not.toBeInTheDocument()
    const topologyItem = screen.getByText('Topology Mode').closest('.ant-form-item')!
    fireEvent.mouseDown(topologyItem.querySelector('input')!)
    fireEvent.click((await screen.findAllByText('sentinel')).at(-1)!)
    const edited = onChange.mock.calls.at(-1)?.[0].spec.config
    expect(edited.endpoints).toEqual([])
    expect(edited.topology).toEqual({ mode: 'sentinel', sentinel: { masterName: '', sentinels: [] }, cluster: undefined })
    expect(edited.db).toBe(255)
  })
})
