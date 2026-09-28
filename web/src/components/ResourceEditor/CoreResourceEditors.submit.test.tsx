import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { load } from 'js-yaml'
import ServiceEditor from './Service/ServiceEditor'
import EndpointSliceEditor from './EndpointSlice/EndpointSliceEditor'

vi.mock('@/components/YamlEditor', () => ({ default: ({value,onChange}:any) => <textarea aria-label="yaml-source" value={value} onChange={(e)=>onChange(e.target.value)} /> }))

describe('core resource editor submissions', () => {
  it('submits the real Service Form path without relying on YAML', async () => {
    const submit=vi.fn().mockResolvedValue(undefined)
    render(<ServiceEditor visible mode="create" onClose={vi.fn()} onSubmit={submit}/>)
    fireEvent.change(screen.getByPlaceholderText('example-route'),{target:{value:'api'}})
    fireEvent.change(screen.getByPlaceholderText('default'),{target:{value:'prod'}})
    fireEvent.click(screen.getByText('Add port'))
    fireEvent.click(screen.getByRole('button',{name:'Save'}))
    await waitFor(()=>expect(submit).toHaveBeenCalledOnce())
    expect(submit.mock.calls[0][0]).toContain('namespace: prod')
    expect(submit.mock.calls[0][1].spec.ports[0].port).toBe(80)
  })

  it('submits a Service with the YAML namespace and structured ports', async () => {
    const submit=vi.fn().mockResolvedValue(undefined)
    render(<ServiceEditor visible mode="create" onClose={vi.fn()} onSubmit={submit}/>)
    fireEvent.click(screen.getByRole('tab',{name:'YAML'}))
    fireEvent.change(screen.getByLabelText('yaml-source'),{target:{value:'apiVersion: v1\nkind: Service\nmetadata: {name: api, namespace: prod}\nspec:\n  type: ClusterIP\n  selector: {app: api}\n  ports: [{name: http, port: 80, targetPort: web}]\n'}})
    fireEvent.click(screen.getByRole('button',{name:'Save'}))
    await waitFor(()=>expect(submit).toHaveBeenCalledOnce())
    expect(submit.mock.calls[0][1].metadata.namespace).toBe('prod')
    expect(submit.mock.calls[0][1].spec.ports[0].targetPort).toBe('web')
  })

  it('submits an EndpointSlice with association label and endpoint conditions', async () => {
    const submit=vi.fn().mockResolvedValue(undefined)
    render(<EndpointSliceEditor visible mode="create" onClose={vi.fn()} onSubmit={submit}/>)
    fireEvent.click(screen.getByRole('tab',{name:'YAML'}))
    fireEvent.change(screen.getByLabelText('yaml-source'),{target:{value:'apiVersion: discovery.k8s.io/v1\nkind: EndpointSlice\nmetadata:\n  name: api-a\n  namespace: edge\n  labels: {kubernetes.io/service-name: api}\naddressType: IPv4\nports: [{name: http, port: 8080}]\nendpoints: [{addresses: [10.0.0.1], conditions: {ready: true}}]\n'}})
    fireEvent.click(screen.getByRole('button',{name:'Save'}))
    await waitFor(()=>expect(submit).toHaveBeenCalledOnce())
    expect(submit.mock.calls[0][1].metadata.namespace).toBe('edge')
    expect(submit.mock.calls[0][1].endpoints[0].conditions.ready).toBe(true)
  })

  it('submits the real EndpointSlice Form path', async () => {
    const submit=vi.fn().mockResolvedValue(undefined)
    render(<EndpointSliceEditor visible mode="create" onClose={vi.fn()} onSubmit={submit}/>)
    fireEvent.change(screen.getByPlaceholderText('example-route'),{target:{value:'api-a'}})
    fireEvent.change(screen.getByPlaceholderText('default'),{target:{value:'edge'}})
    const serviceItem=screen.getByText('Service name').closest('.ant-form-item')!
    fireEvent.change(serviceItem.querySelector('input')!,{target:{value:'api'}})
    fireEvent.click(screen.getByText('Add endpoint'))
    fireEvent.change(screen.getByPlaceholderText('hostname'),{target:{value:'api-1'}})
    const addressItem=screen.getByText('Addresses (comma separated)').closest('.ant-form-item')!
    fireEvent.change(addressItem.querySelector('input')!,{target:{value:'10.0.0.1'}})
    fireEvent.click(screen.getByRole('button',{name:'Save'}))
    await waitFor(()=>expect(submit).toHaveBeenCalledOnce())
    expect(submit.mock.calls[0][0]).toContain('kubernetes.io/service-name: api')
    expect(submit.mock.calls[0][1].endpoints[0].addresses).toEqual(['10.0.0.1'])
  })
})


it.each([['', undefined], ['9090', 9090], ['web', 'web']])('preserves Service fields while editing targetPort to %j', async (input, expected) => {
  const submit = vi.fn().mockResolvedValue(undefined)
  const resource = { apiVersion: 'v1', kind: 'Service', metadata: { name: 'api', namespace: 'app', resourceVersion: '9' }, spec: {
    ports: [{ name: 'http', port: 80, targetPort: 8080, appProtocol: 'http', nodePort: 30080 }, { name: 'metrics', port: 9091, targetPort: 'metrics' }],
    ipFamilyPolicy: 'SingleStack', futureField: { retain: true },
  }, status: { loadBalancer: {} } }
  render(<ServiceEditor visible mode="edit" resource={resource} onClose={vi.fn()} onSubmit={submit} />)
  fireEvent.change(screen.getAllByPlaceholderText('targetPort')[0], { target: { value: input } })
  fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
  const switched = load((screen.getByLabelText('yaml-source') as HTMLTextAreaElement).value) as any
  if (expected === undefined) expect(switched.spec.ports[0]).not.toHaveProperty('targetPort')
  else expect(switched.spec.ports[0].targetPort).toBe(expected)
  fireEvent.click(screen.getByRole('tab', { name: 'Form' }))
  fireEvent.click(screen.getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(submit).toHaveBeenCalledOnce())
  const result = load(submit.mock.calls[0][0]) as any
  expect(result.spec.ports[0]).toMatchObject({ name: 'http', port: 80, appProtocol: 'http', nodePort: 30080 })
  expect(result.spec.ports[1]).toEqual(resource.spec.ports[1])
  expect(result.spec.futureField).toEqual({ retain: true })
  expect(result.spec.ipFamilyPolicy).toBe('SingleStack')
  expect(result.metadata.resourceVersion).toBe('9')
  expect(result).not.toHaveProperty('status')
  if (expected === undefined) expect(result.spec.ports[0]).not.toHaveProperty('targetPort')
  else expect(result.spec.ports[0].targetPort).toBe(expected)
})
