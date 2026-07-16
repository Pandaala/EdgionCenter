import { AxiosError } from 'axios'
import { message } from 'antd'
import { describe, expect, it, vi } from 'vitest'
import type { AxiosResponse, InternalAxiosRequestConfig } from 'axios'
import { apiClient, conflictErrorMessage, isCreateRequest, shouldApplyControllerProxy } from './client'

describe('controller proxy URL classification', () => {
  it('rewrites relative Controller API paths', () => {
    expect(shouldApplyControllerProxy('namespaced/httproute')).toBe(true)
    expect(shouldApplyControllerProxy('/namespaced/httproute')).toBe(true)
    expect(shouldApplyControllerProxy('/cluster/gatewayclass')).toBe(true)
  })

  it('preserves explicit rooted Center and proxy paths', () => {
    expect(shouldApplyControllerProxy('/api/v1/center/admin/controllers')).toBe(false)
    expect(shouldApplyControllerProxy('/api/v1/proxy/e2e-a~controller-a/api/v1/access')).toBe(false)
  })

  it('resets the instance base URL for an explicit rooted proxy request', async () => {
    let observedBaseURL: string | undefined
    await apiClient.get('/api/v1/proxy/e2e-a~controller-a/api/v1/access', {
      _skipControllerProxy: true,
      adapter: async (config: InternalAxiosRequestConfig) => {
        observedBaseURL = config.baseURL
        return { data: null, status: 200, statusText: 'OK', headers: {}, config }
      },
    } as any)

    expect(observedBaseURL).toBe('/')
  })
})

describe('conflict guidance', () => {
  it('classifies only resource and admin collection POSTs as creates', () => {
    expect(isCreateRequest('post', '/namespaced/httproute/default')).toBe(true)
    expect(isCreateRequest('post', '/cluster/gatewayclass')).toBe(true)
    expect(isCreateRequest('post', 'center/admin/users')).toBe(true)
    expect(isCreateRequest('post', 'reload')).toBe(false)
    expect(isCreateRequest('post', '/services/acme/default/cert/trigger')).toBe(false)
    expect(isCreateRequest('post', 'center/region-routes/failover')).toBe(false)
  })

  it('distinguishes create collisions, stale mutations, and action conflicts', () => {
    expect(conflictErrorMessage('post', '/namespaced/httproute/default')).toContain('already exists')
    expect(conflictErrorMessage('put', '/namespaced/httproute/default/route')).toContain('refresh and retry')
    expect(conflictErrorMessage('delete', '/cluster/gatewayclass/class')).toContain('refresh and retry')
    expect(conflictErrorMessage('post', 'reload')).toContain('Request conflict')
  })

  it('uses URL-aware guidance in the installed response interceptor', async () => {
    const errorSpy = vi.spyOn(message, 'error').mockImplementation(() => undefined as any)
    const conflictAdapter = async (config: InternalAxiosRequestConfig): Promise<AxiosResponse> => {
      const response = { data: { error: 'not leader' }, status: 409, statusText: 'Conflict', headers: {}, config }
      throw new AxiosError('conflict', 'ERR_BAD_REQUEST', config, undefined, response)
    }

    await expect(apiClient.post('reload', undefined, { adapter: conflictAdapter })).rejects.toBeInstanceOf(AxiosError)
    expect(errorSpy).toHaveBeenCalledWith(expect.stringContaining('Request conflict'))
    expect(errorSpy).not.toHaveBeenCalledWith(expect.stringContaining('already exists'))
    errorSpy.mockRestore()
  })
})
