import { createElement, type ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useResourceList } from './useResourceList'
const listAll=vi.hoisted(()=>vi.fn())
vi.mock('@/api/resources',()=>({resourceApi:{listAll:(...args:unknown[])=>listAll(...args),list:vi.fn()},clusterResourceApi:{listAll:vi.fn()}}))
const wrapper=({children}:{children:ReactNode})=>createElement(QueryClientProvider,{client:new QueryClient({defaultOptions:{queries:{retry:false}}})},children)
describe('useResourceList authorization gate',()=>{
  beforeEach(()=>listAll.mockReset().mockResolvedValue({success:true,count:0,data:[]}))
  it('issues no request while fail-closed and starts after access is confirmed',async()=>{
    const {rerender}=renderHook(({enabled})=>useResourceList('service',{namespaced:true,enabled}),{wrapper,initialProps:{enabled:false}})
    expect(listAll).not.toHaveBeenCalled()
    rerender({enabled:true})
    await waitFor(()=>expect(listAll).toHaveBeenCalledOnce())
  })
})
