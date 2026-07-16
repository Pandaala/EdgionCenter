import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import BackendTLSPolicyEditor from './BackendTLSPolicyEditor'
const api=vi.hoisted(()=>({create:vi.fn()}))
vi.mock('@/api/resources',()=>({resourceApi:{create:(...args:unknown[])=>api.create(...args),update:vi.fn()}}))
vi.mock('@/components/YamlEditor',()=>({default:({value,onChange}:any)=><textarea aria-label="yaml-source" value={value} onChange={(e)=>onChange(e.target.value)}/> }))
const renderEditor=()=>render(<QueryClientProvider client={new QueryClient()}><BackendTLSPolicyEditor visible mode="create" onClose={vi.fn()}/></QueryClientProvider>)
describe('BackendTLSPolicy request payloads',()=>{
  beforeEach(()=>api.create.mockReset().mockResolvedValue({success:true}))
  it('submits the real Form path with same-namespace CA and bare client cert',async()=>{
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText('example-route'),{target:{value:'api-tls'}})
    fireEvent.change(screen.getByPlaceholderText('my-backend-service'),{target:{value:'api'}})
    fireEvent.change(screen.getByPlaceholderText('backend.internal'),{target:{value:'api.internal'}})
    fireEvent.change(screen.getByPlaceholderText('backend-ca'),{target:{value:'ca'}})
    const certItem=screen.getByText('Same namespace only. Enter a bare Secret name, never namespace/name.').closest('.ant-form-item')!
    fireEvent.change(certItem.querySelector('input')!,{target:{value:'client-cert'}})
    fireEvent.click(screen.getByRole('button',{name:'Create'}))
    await waitFor(()=>expect(api.create).toHaveBeenCalledOnce())
    expect(api.create.mock.calls[0][3]).not.toMatch(/caCertificateRefs:[\s\S]*namespace:/)
    expect(api.create.mock.calls[0][3]).toContain('edgion.io/client-certificate-ref: client-cert')
  })
  it('submits the YAML path through validation and mutation stripping',async()=>{
    renderEditor();fireEvent.click(screen.getByRole('tab',{name:'YAML'}))
    fireEvent.change(screen.getByLabelText('yaml-source'),{target:{value:'apiVersion: gateway.networking.k8s.io/v1\nkind: BackendTLSPolicy\nmetadata: {name: api, namespace: prod}\nspec:\n  targetRefs: [{group: "", kind: Service, name: api}]\n  validation: {hostname: api.internal, wellKnownCACertificates: System}\n'}})
    fireEvent.click(screen.getByRole('button',{name:'Create'}))
    await waitFor(()=>expect(api.create).toHaveBeenCalledWith(
      { controllerId: null },
      'backendtlspolicy',
      'prod',
      expect.stringContaining('wellKnownCACertificates: System'),
    ))
  })
})
