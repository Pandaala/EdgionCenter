import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { configMapFromYaml, configMapToYaml, createConfigMapReplacement } from './configmap'
import { normalizeService, serviceToYaml, validateService } from './service'
import { normalizeEndpointSlice, endpointSliceToYaml, validateEndpointSlice } from './endpointslice'

describe('core Kubernetes resource adapters', () => {
  it('preserves unknown Service fields but removes server-owned metadata/status', () => {
    const service = normalizeService({ apiVersion:'v1', kind:'Service', metadata:{name:'api',namespace:'prod',resourceVersion:'9'}, spec:{type:'ClusterIP',selector:{app:'api'},ports:[{name:'http',port:80,targetPort:'web'}],ipFamilyPolicy:'SingleStack'}, status:{loadBalancer:{}} })
    validateService(service)
    const output = yaml.load(serviceToYaml(service, 'update')) as any
    expect(output.spec.ipFamilyPolicy).toBe('SingleStack')
    expect(output.metadata.resourceVersion).toBe('9')
    expect(output.status).toBeUndefined()
  })

  it('uses the submitted EndpointSlice namespace and preserves endpoint fields', () => {
    const slice = normalizeEndpointSlice({ apiVersion:'discovery.k8s.io/v1',kind:'EndpointSlice',metadata:{name:'api-a',namespace:'edge',labels:{'kubernetes.io/service-name':'api'}},addressType:'IPv4',ports:[{name:'http',port:8080}],endpoints:[{addresses:['10.0.0.1'],conditions:{ready:true},zone:'z1',hints:{forZones:[{name:'z1'}]}}] })
    validateEndpointSlice(slice)
    const output = yaml.load(endpointSliceToYaml(slice, 'update')) as any
    expect(output.metadata.namespace).toBe('edge')
    expect(output.endpoints[0].hints.forZones[0].name).toBe('z1')
  })

  it('supports ConfigMap data, binaryData and immutable losslessly', () => {
    const value = configMapFromYaml('apiVersion: v1\nkind: ConfigMap\nmetadata: {name: cfg, namespace: prod}\ndata: {a: ""}\nbinaryData: {blob: YQ==}\nimmutable: false\n')
    expect(yaml.load(configMapToYaml(value, 'create'))).toMatchObject({ data:{a:''}, binaryData:{blob:'YQ=='}, immutable:false })
  })

  it('keeps the metadata-only ConfigMap replacement precondition', () => {
    const replacement = createConfigMapReplacement({ name: 'cfg', namespace: 'prod', resourceVersion: '21' })
    expect(replacement.metadata.resourceVersion).toBe('21')
    expect(configMapToYaml(replacement, 'update')).toContain("resourceVersion: '21'")
  })
})
