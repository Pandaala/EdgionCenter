import { apiClient } from './client'

// ===== Type definitions (mirroring backend types) =====

export interface IpGroup {
  name: string
  description?: string
  cidrs: string[]
}

export interface ProfileRules {
  allow?: IpGroup[]
  deny?: IpGroup[]
  defaultAction: 'allow' | 'deny'
}

/** Per-controller effective view returned by the backend (camelCase). */
export interface EffectiveGirView {
  namespace: string
  pluginName: string
  enable: boolean
  activeProfile: string
  profiles: Record<string, ProfileRules>
  activeProfileRef?: string
  selectorApplied: boolean
}

/** Aggregated list item returned by GET /center/global-connection-ip-restrictions. */
export interface CenterGirAggregatedView {
  namespace: string
  pluginName: string
  controllers: Record<string, EffectiveGirView>
  onlineControllerIds: string[]
}

export interface ControllerOpResult {
  controllerId: string
  detail?: string
  error?: string
  statusCode?: number
}

export interface FanOutResponse {
  success: ControllerOpResult[]
  failed: ControllerOpResult[]
  warnings: string[]
}

export interface ConsistencyResult {
  namespace: string
  name: string
  consistent: boolean
  controllerCount: number
  conflicts: string[]
}

// ===== API =====

const BASE = 'center/global-connection-ip-restrictions'

export const globalConnectionIpRestrictionApi = {
  list: async (): Promise<{ success: boolean; data: CenterGirAggregatedView[] }> => {
    const { data } = await apiClient.get(BASE)
    return data
  },

  get: async (namespace: string, name: string): Promise<{ success: boolean; data: CenterGirAggregatedView }> => {
    const { data } = await apiClient.get(`${BASE}/${namespace}/${name}`)
    return data
  },

  consistency: async (): Promise<{ success: boolean; data: ConsistencyResult[] }> => {
    const { data } = await apiClient.get(`${BASE}/consistency`)
    return data
  },

  patchActiveProfile: async (
    namespace: string,
    name: string,
    activeProfile: string,
    controllers: string[]
  ): Promise<{ success: boolean; data: FanOutResponse }> => {
    const { data } = await apiClient.patch(`${BASE}/${namespace}/${name}/active-profile`, {
      activeProfile,
      controllers,
    })
    return data
  },
}
