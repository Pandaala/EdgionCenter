import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SecretEditor from './Secret/SecretEditor'
import ConfigMapEditor from './ConfigMap/ConfigMapEditor'

const api=vi.hoisted(()=>({create:vi.fn(),update:vi.fn()}))
vi.mock('@/api/resources',()=>({resourceApi:{create:(...args:unknown[])=>api.create(...args),update:(...args:unknown[])=>api.update(...args)}}))
vi.mock('@/components/YamlEditor',()=>({default:({value,onChange}:any)=><textarea aria-label="yaml-source" value={value} onChange={(e)=>onChange(e.target.value)}/> }))
function wrapper(node:React.ReactNode){return render(<QueryClientProvider client={new QueryClient({defaultOptions:{mutations:{retry:false}}})}>{node}</QueryClientProvider>)}

describe('restricted dependency request payloads',()=>{
  beforeEach(()=>{api.create.mockReset().mockResolvedValue({success:true});api.update.mockReset().mockResolvedValue({success:true})})
  it('submits Secret Form values only through write-only stringData',async()=>{
    wrapper(<SecretEditor visible mode="create" onClose={vi.fn()}/>)
    fireEvent.change(screen.getByPlaceholderText('example-route'),{target:{value:'credentials'}})
    fireEvent.click(screen.getByText('Add Data Entry'))
    fireEvent.change(screen.getByPlaceholderText('Base64 encoded value'),{target:{value:'new-secret'}})
    fireEvent.click(screen.getByRole('button',{name:'Create'}))
    await waitFor(()=>expect(api.create).toHaveBeenCalledOnce())
    expect(api.create.mock.calls[0][3]).toContain('stringData:')
    expect(api.create.mock.calls[0][3]).not.toMatch(/^data:/m)
  })
  it('submits Secret YAML and rejects no write-only values at the same boundary',async()=>{
    wrapper(<SecretEditor visible mode="create" onClose={vi.fn()}/>)
    fireEvent.click(screen.getByRole('tab',{name:'YAML'}))
    fireEvent.change(screen.getByLabelText('yaml-source'),{target:{value:'apiVersion: v1\nkind: Secret\nmetadata: {name: token, namespace: prod}\ntype: Opaque\nstringData: {token: fresh}\n'}})
    fireEvent.click(screen.getByRole('button',{name:'Create'}))
    await waitFor(()=>expect(api.create).toHaveBeenCalledWith(
      { controllerId: null },
      'secret',
      'prod',
      expect.stringContaining('token: fresh'),
    ))
  })
  it('submits ConfigMap Form data',async()=>{
    wrapper(<ConfigMapEditor visible mode="create" onClose={vi.fn()}/>)
    fireEvent.change(screen.getByPlaceholderText('example-route'),{target:{value:'settings'}})
    fireEvent.click(screen.getAllByText('Add entry')[0])
    const textareas=screen.getAllByRole('textbox').filter((node)=>node.tagName==='TEXTAREA')
    fireEvent.change(textareas[0],{target:{value:'enabled'}})
    fireEvent.click(screen.getByRole('button',{name:'Create'}))
    await waitFor(()=>expect(api.create).toHaveBeenCalledOnce())
    expect(api.create.mock.calls[0][3]).toContain('data:')
    expect(api.create.mock.calls[0][3]).toContain('enabled')
  })
  it('submits ConfigMap YAML with binaryData',async()=>{
    wrapper(<ConfigMapEditor visible mode="create" onClose={vi.fn()}/>)
    fireEvent.click(screen.getByRole('tab',{name:'YAML'}))
    fireEvent.change(screen.getByLabelText('yaml-source'),{target:{value:'apiVersion: v1\nkind: ConfigMap\nmetadata: {name: bin, namespace: prod}\nbinaryData: {blob: YQ==}\n'}})
    fireEvent.click(screen.getByRole('button',{name:'Create'}))
    await waitFor(()=>expect(api.create).toHaveBeenCalledWith(
      { controllerId: null },
      'configmap',
      'prod',
      expect.stringContaining('blob: YQ=='),
    ))
  })
})
