import { fireEvent, screen, waitFor } from '@testing-library/react'
import { Modal } from 'antd'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { renderWithQueryClient } from '@/test/render'
import ServiceList from './Infrastructure/ServiceList'
import EndpointSliceList from './Infrastructure/EndpointSliceList'
import BackendTLSPolicyList from './Security/BackendTLSPolicyList'
const batchDelete=vi.hoisted(()=>vi.fn())
vi.mock('@/api/resources',()=>({
  batchDeleteFailureKeys: (error: unknown) => (error as { failedKeys?: string[] }).failedKeys ?? null,
  resourceApi:{delete:vi.fn(),batchDelete:(...args:unknown[])=>batchDelete(...args)},
}))
vi.mock('@/hooks/useControllerAccess',()=>({useControllerAccess:()=>({canResource:()=>true})}))
vi.mock('@/components/resource/PermissionAwareButton',()=>({default:(props:any)=><button onClick={props.onClick}>{props.children}</button>}))
vi.mock('@/hooks/useResourceList',()=>({useResourceList:(kind:string)=>({items:[{apiVersion:'v1',kind,metadata:{namespace:'prod',name:`${kind}-a`},spec:{}},{apiVersion:'v1',kind,metadata:{namespace:'prod',name:`${kind}-b`},spec:{}}],isLoading:false,error:null,refetch:vi.fn(),fetchNextPage:vi.fn(),hasNextPage:false,isFetchingNextPage:false})}))
vi.mock('@/components/ResourceEditor/Service/ServiceEditor',()=>({default:()=>null}))
vi.mock('@/components/ResourceEditor/EndpointSlice/EndpointSliceEditor',()=>({default:()=>null}))
vi.mock('@/components/ResourceEditor/BackendTLSPolicy/BackendTLSPolicyEditor',()=>({default:()=>null}))
vi.mock('react-router-dom',async()=>{const actual=await vi.importActual<typeof import('react-router-dom')>('react-router-dom');return {...actual,useParams:()=>({controllerId:'cluster/ctrl'})}})
const cases=[['service',ServiceList],['endpointslice',EndpointSliceList],['backendtlspolicy',BackendTLSPolicyList]] as const
describe('permission-aware selected-set batch deletion',()=>{
  beforeEach(()=>{batchDelete.mockReset().mockResolvedValue(undefined);vi.spyOn(Modal,'confirm').mockImplementation((config)=>{void config.onOk?.();return {destroy:vi.fn(),update:vi.fn()}})})
  it.each(cases)('%s deletes the complete selected set',(kind,Component)=>{
    renderWithQueryClient(<Component />)
    const boxes=screen.getAllByRole('checkbox');fireEvent.click(boxes[1]);fireEvent.click(boxes[2])
    fireEvent.click(screen.getByRole('button',{name:'Batch Delete'}))
    return waitFor(()=>expect(batchDelete).toHaveBeenCalledWith(
      { controllerId: 'cluster/ctrl' },
      kind,
      [{namespace:'prod',name:`${kind}-a`},{namespace:'prod',name:`${kind}-b`}],
    ))
  })

  it('keeps only failed rows selected after a partial batch delete', async () => {
    batchDelete.mockRejectedValueOnce(Object.assign(new Error('one delete failed'), {
      failedKeys: ['prod/service-b'],
    }))
    renderWithQueryClient(<ServiceList />)
    const boxes = screen.getAllByRole('checkbox')
    fireEvent.click(boxes[1])
    fireEvent.click(boxes[2])
    fireEvent.click(screen.getByRole('button', { name: 'Batch Delete' }))

    await waitFor(() => {
      expect(boxes[1]).not.toBeChecked()
      expect(boxes[2]).toBeChecked()
    })
  })
})
