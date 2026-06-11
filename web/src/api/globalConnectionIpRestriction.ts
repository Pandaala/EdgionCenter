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

export interface GlobalConnectionIpRestrictionData {
  enable: boolean
  activeProfile: string
  profiles: Record<string, ProfileRules>
  description?: string
}

export interface ControllerPmEntry {
  pmNamespace: string
  pmName: string
  enable: boolean
  activeProfile: string
  profiles: Record<string, ProfileRules>
  description?: string
  contentHash: string
  lastModified: number
}

export interface CenterGlobalIpRestrictionView {
  namespace: string
  name: string
  controllers: Record<string, ControllerPmEntry>
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
  list: async (): Promise<{ success: boolean; data: CenterGlobalIpRestrictionView[] }> => {
    const { data } = await apiClient.get(BASE)
    return data
  },

  get: async (namespace: string, name: string): Promise<{ success: boolean; data: CenterGlobalIpRestrictionView }> => {
    const { data } = await apiClient.get(`${BASE}/${namespace}/${name}`)
    return data
  },

  consistency: async (): Promise<{ success: boolean; data: ConsistencyResult[] }> => {
    const { data } = await apiClient.get(`${BASE}/consistency`)
    return data
  },

  create: async (req: {
    namespace: string
    name: string
    controllers: string[]
    data: GlobalConnectionIpRestrictionData
  }): Promise<{ success: boolean; data: FanOutResponse }> => {
    const { data } = await apiClient.post(BASE, req)
    return data
  },

  update: async (
    namespace: string,
    name: string,
    req: { controllers: string[]; data: GlobalConnectionIpRestrictionData }
  ): Promise<{ success: boolean; data: FanOutResponse }> => {
    const { data } = await apiClient.put(`${BASE}/${namespace}/${name}`, req)
    return data
  },

  delete_: async (
    namespace: string,
    name: string,
    controllers: string[]
  ): Promise<{ success: boolean; data: FanOutResponse }> => {
    const { data } = await apiClient.delete(`${BASE}/${namespace}/${name}`, {
      data: { controllers },
    })
    return data
  },

  patchEnable: async (
    namespace: string,
    name: string,
    enable: boolean,
    controllers: string[]
  ): Promise<{ success: boolean; data: FanOutResponse }> => {
    const { data } = await apiClient.patch(`${BASE}/${namespace}/${name}/enable`, {
      enable,
      controllers,
    })
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

  sync: async (
    namespace: string,
    name: string,
    sourceController: string,
    targetControllers: string[]
  ): Promise<{ success: boolean; data: FanOutResponse }> => {
    const { data } = await apiClient.post(`${BASE}/${namespace}/${name}/sync`, {
      sourceController,
      targetControllers,
    })
    return data
  },
}
