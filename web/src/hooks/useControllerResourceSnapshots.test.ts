import { beforeEach, describe, expect, it, vi } from 'vitest'
import { listFirstClassResources } from '@/config/resourceCatalog'
import { loadControllerResourceSnapshot } from './useControllerResourceSnapshots'

const mocks = vi.hoisted(() => ({
  getAccess: vi.fn(),
  listControllerResources: vi.fn(),
}))

vi.mock('@/api/access', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/access')>()
  return {
    ...actual,
    controllerAccessApi: { get: mocks.getAccess },
  }
})
vi.mock('@/api/center', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/center')>()
  return {
    ...actual,
    centerApi: {
      ...actual.centerApi,
      listControllerResources: mocks.listControllerResources,
    },
  }
})

const controller = {
  controller_id: 'edge/controller-a',
  cluster: 'edge',
  env: [],
  tag: [],
  online: true,
  key_count: null,
}

describe('loadControllerResourceSnapshot', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.listControllerResources.mockResolvedValue({ success: true, data: [] })
  })

  it('falls back to the bounded first-class catalog when an older Controller has no access endpoint', async () => {
    mocks.getAccess.mockRejectedValue(new Error('404'))

    const snapshot = await loadControllerResourceSnapshot(controller)

    expect(mocks.listControllerResources).toHaveBeenCalledTimes(listFirstClassResources().length)
    expect(snapshot.errors).toEqual([])
  })

  it('records only concrete list failures during the compatibility fallback', async () => {
    mocks.getAccess.mockRejectedValue(new Error('404'))
    mocks.listControllerResources.mockImplementation(
      async (_id: string, kind: string) => {
        if (kind === 'httproute') throw new Error('not supported')
        return { success: true, data: [] }
      },
    )

    const snapshot = await loadControllerResourceSnapshot(controller)

    expect(snapshot.errors).toEqual(['httproute'])
  })
})
