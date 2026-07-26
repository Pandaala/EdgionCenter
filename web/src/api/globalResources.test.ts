import { afterEach, describe, expect, it, vi } from 'vitest'
import { apiClient } from './client'
import {
  globalResourcesApi,
  type GlobalResourceListResponse,
} from './globalResources'
import {
  findGlobalResourceDescriptor,
  GLOBAL_RESOURCE_DESCRIPTORS,
} from '@/pages/GlobalResources/globalResourceDescriptors'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('Global Resources Center API', () => {
  it('uses explicit Center paths and always skips the Controller proxy', async () => {
    const get = vi.spyOn(apiClient, 'get').mockResolvedValue({ data: {} } as never)

    await globalResourcesApi.catalog()
    await globalResourcesApi.detail(
      'http-route',
      'edgion system',
      'route/name',
      'cluster-a',
    )
    await globalResourcesApi.preflight('cluster-b')

    expect(get).toHaveBeenNthCalledWith(
      1,
      '/api/v1/center/global-resources/catalog',
      expect.objectContaining({ _skipControllerProxy: true }),
    )
    expect(get).toHaveBeenNthCalledWith(
      2,
      '/api/v1/center/global-resources/resources/http-route/edgion%20system/route%2Fname',
      expect.objectContaining({ _skipControllerProxy: true }),
    )
    expect(get).toHaveBeenNthCalledWith(
      3,
      '/api/v1/center/global-resources/preflight',
      expect.objectContaining({ _skipControllerProxy: true }),
    )

    const detailParams = get.mock.calls[1][1]?.params as URLSearchParams
    const preflightParams = get.mock.calls[2][1]?.params as URLSearchParams
    expect([...detailParams.entries()]).toEqual([['cluster', 'cluster-a']])
    expect([...preflightParams.entries()]).toEqual([['cluster', 'cluster-b']])
  })

  it('encodes repeated clusters, exact ConfigData type, limit, and continuation', async () => {
    const response: GlobalResourceListResponse = {
      catalogRevision: 'sha256:catalog',
      configRevision: 'sha256:config',
      membershipRevision: 'sha256:membership',
      inventoryRevision: 'sha256:inventory',
      kind: 'EdgionConfigData',
      configDataType: 'IpList',
      clusters: [
        {
          cluster: 'cluster-a',
          state: 'offline',
          controllerId: null,
          candidates: ['cluster-a/controller-a'],
          complete: false,
          errors: [
            {
              namespace: 'edgion-data',
              code: 'controller_offline',
              status: null,
              retryable: true,
            },
          ],
        },
      ],
      groups: [],
      continueToken: 'gr1.opaque',
    }
    const get = vi.spyOn(apiClient, 'get').mockResolvedValue({ data: response } as never)

    await expect(globalResourcesApi.list('edgion-config-data', {
      clusters: ['cluster-a', 'cluster-b'],
      configDataType: 'IpList',
      limit: 25,
      continueToken: 'gr1.previous+opaque',
    })).resolves.toBe(response)

    expect(get).toHaveBeenCalledWith(
      '/api/v1/center/global-resources/resources/edgion-config-data',
      expect.objectContaining({ _skipControllerProxy: true }),
    )
    const params = get.mock.calls[0][1]?.params as URLSearchParams
    expect([...params.entries()]).toEqual([
      ['cluster', 'cluster-a'],
      ['cluster', 'cluster-b'],
      ['configDataType', 'IpList'],
      ['limit', '25'],
      ['continue', 'gr1.previous+opaque'],
    ])
  })
})

describe('Global Resource descriptors', () => {
  it('covers every route and exact EdgionConfigData type once', () => {
    expect(GLOBAL_RESOURCE_DESCRIPTORS).toHaveLength(9)
    expect(new Set(GLOBAL_RESOURCE_DESCRIPTORS.map(({ route }) => route)).size).toBe(9)
    expect(
      GLOBAL_RESOURCE_DESCRIPTORS
        .filter(({ kind }) => kind === 'EdgionConfigData')
        .map((descriptor) => (
          'configDataType' in descriptor ? descriptor.configDataType : undefined
        )),
    ).toEqual([
      'KeyList',
      'IpList',
      'Selector',
      'RegionRouteOverride',
      'Misc',
    ])
    expect(
      GLOBAL_RESOURCE_DESCRIPTORS
        .filter(({ kind }) => kind !== 'EdgionConfigData')
        .every((descriptor) => !('configDataType' in descriptor)),
    ).toBe(true)
  })

  it('resolves only exact descriptor routes', () => {
    expect(
      findGlobalResourceDescriptor('/global-resources/edgion-config-data/ip-list'),
    ).toMatchObject({
      kind: 'EdgionConfigData',
      apiSlug: 'edgion-config-data',
      configDataType: 'IpList',
    })
    expect(
      findGlobalResourceDescriptor('/global-resources/edgion-config-data/ip-list/extra'),
    ).toBeUndefined()
  })
})
