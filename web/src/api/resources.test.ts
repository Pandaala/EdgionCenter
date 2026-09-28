import { afterEach, describe, expect, it, vi } from 'vitest'
import { apiClient } from './client'
import { controllerAccessApi } from './access'
import {
  BatchDeletePartialError,
  clusterResourceApi,
  resourceApi,
  type ControllerMutationTarget,
} from './resources'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('resource mutation execution boundary', () => {
  it('pins ACME dispatch and treats success as queue admission only', async () => {
    const post = vi.spyOn(apiClient, 'post').mockResolvedValue({ data: { success: true } })
    expect(await resourceApi.triggerAcme({ controllerId: 'east/controller' }, 'app', 'cert')).toBe('queued')
    expect(post).toHaveBeenCalledWith('/services/acme/app/cert/trigger', undefined,
      expect.objectContaining({ baseURL: '/api/v1/proxy/east~controller/api/v1', _skipControllerProxy: true, _silent: true }))
  })

  it.each([[403, 'denied'], [503, 'unavailable'], [404, 'failed'], [502, 'unknown'], [504, 'unknown'], [undefined, 'unknown']])(
    'classifies ACME dispatch failure %s without retrying', async (status, expected) => {
      const post = vi.spyOn(apiClient, 'post').mockRejectedValue({ isAxiosError: true,
        response: status ? { status, data: { error: 'private remote detail' } } : undefined })
      expect(await resourceApi.triggerAcme({ controllerId: null }, 'app', 'cert')).toBe(expected)
      expect(post).toHaveBeenCalledTimes(1)
    })

  it('rejects an unsuccessful ACME response envelope', async () => {
    vi.spyOn(apiClient, 'post').mockResolvedValue({ data: { success: false, error: 'private remote detail' } })
    expect(await resourceApi.triggerAcme({ controllerId: null }, 'app', 'cert')).toBe('failed')
  })

  it('pins source list reads to the captured Controller across all scopes', async () => {
    const get = vi.spyOn(apiClient, 'get').mockResolvedValue({ data: { success: true, data: [], count: 0 } })
    const target = { controllerId: 'east/controller' }
    await resourceApi.listAll('httproute', { limit: 20, target })
    await resourceApi.list('httproute', 'app', target)
    await clusterResourceApi.listAll('gatewayclass', { target })
    expect(get.mock.calls.map(([url]) => url)).toEqual([
      '/namespaced/httproute?limit=20', '/namespaced/httproute/app', '/cluster/gatewayclass',
    ])
    for (const [, config] of get.mock.calls) {
      expect(config).toMatchObject({ baseURL: '/api/v1/proxy/east~controller/api/v1', _skipControllerProxy: true })
    }
  })

  it('reads processed status from the captured Controller without the CRUD API prefix', async () => {
    const get = vi.spyOn(apiClient, 'get').mockResolvedValue({
      data: { success: true, data: { kind: 'HTTPRoute', metadata: { name: 'route' }, status: { parents: [] } } },
    })

    const result = await resourceApi.getProcessed(
      { controllerId: 'east/controller-1' },
      'httproute',
      'prod',
      'route',
    )

    expect(result.status).toEqual({ parents: [] })
    expect(get).toHaveBeenCalledWith(
      '/configserver/httproute?name=route&namespace=prod',
      expect.objectContaining({
        baseURL: '/api/v1/proxy/east~controller-1',
        _skipControllerProxy: true,
        _silent: true,
      }),
    )
  })

  it('uses the captured controller target and sends the write as the only request', async () => {
    // The Controller enforces the policy on the write itself. Re-reading
    // /api/v1/access first would only double the round trips.
    const access = vi.spyOn(controllerAccessApi, 'get')
    const post = vi.spyOn(apiClient, 'post').mockResolvedValue({ data: { success: true } })
    const target: ControllerMutationTarget = Object.freeze({ controllerId: 'east/controller-1' })

    await resourceApi.create(target, 'service', 'prod', 'kind: Service')

    expect(access).not.toHaveBeenCalled()
    expect(post).toHaveBeenCalledWith(
      '/namespaced/service/prod',
      'kind: Service',
      expect.objectContaining({
        baseURL: '/api/v1/proxy/east~controller-1/api/v1',
        _skipControllerProxy: true,
      }),
    )
  })

  it('preserves resourceVersion in YAML and forwards it as If-Match', async () => {
    const put = vi.spyOn(apiClient, 'put').mockResolvedValue({ data: { success: true } })
    const source = 'apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n  namespace: prod\n  resourceVersion: "42"\nspec: {}\n'

    await resourceApi.update({ controllerId: null }, 'service', 'prod', 'api', source)

    expect(put).toHaveBeenCalledWith(
      '/namespaced/service/prod/api',
      source,
      expect.objectContaining({
        headers: {
          'Content-Type': 'application/yaml',
          'If-Match': '"42"',
        },
      }),
    )
  })

  it('requires and forwards resourceVersion for conditional deletion', async () => {
    const remove = vi.spyOn(apiClient, 'delete').mockResolvedValue({ data: { success: true } })

    await resourceApi.delete({ controllerId: null }, 'service', 'prod', 'api', '43')

    expect(remove).toHaveBeenCalledWith(
      '/namespaced/service/prod/api',
      expect.objectContaining({ headers: { 'If-Match': '"43"' } }),
    )
    await expect(resourceApi.delete({ controllerId: null }, 'service', 'prod', 'api', ''))
      .rejects.toThrow('resourceVersion is required')
  })

  it('bounds batch concurrency and reports each success and failure', async () => {
    let active = 0
    let maximum = 0
    const remove = vi.spyOn(resourceApi, 'delete').mockImplementation(async (_target, _kind, _namespace, name) => {
      active += 1
      maximum = Math.max(maximum, active)
      await Promise.resolve()
      active -= 1
      if (name === 'bad') throw new Error('conflict')
      return { success: true }
    })
    const resources = ['one', 'two', 'bad', 'four', 'five'].map((name) => ({ namespace: 'prod', name, resourceVersion: '7' }))

    const error = await resourceApi.batchDelete({ controllerId: null }, 'service', resources, 2)
      .catch((reason: unknown) => reason)

    expect(remove).toHaveBeenCalledTimes(5)
    expect(maximum).toBeLessThanOrEqual(2)
    expect(error).toBeInstanceOf(BatchDeletePartialError)
    expect((error as BatchDeletePartialError).result.succeeded).toHaveLength(4)
    expect((error as BatchDeletePartialError).result.failed.map(({ name }) => name)).toEqual(['bad'])
  })
})
