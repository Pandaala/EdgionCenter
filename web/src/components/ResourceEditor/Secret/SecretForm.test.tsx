import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import SecretForm from './SecretForm'
import type { SecretResource } from '@/utils/secret'

describe('SecretForm type switching',()=>{
  it('clears incompatible stringData keys and never resurrects data',async()=>{
    const onChange=vi.fn()
    const value:SecretResource={apiVersion:'v1',kind:'Secret',metadata:{name:'s',namespace:'prod'},type:'Opaque',stringData:{legacy:'private'}}
    render(<SecretForm data={value} onChange={onChange}/>)
    fireEvent.mouseDown(screen.getByRole('combobox'))
    fireEvent.click((await screen.findAllByText('kubernetes.io/tls')).at(-1)!)
    const next=onChange.mock.calls.at(-1)![0]
    expect(next.data).toBeUndefined()
    expect(next.stringData).toEqual({'tls.crt':'','tls.key':''})
    expect(next.stringData).not.toHaveProperty('legacy')
  })
})
