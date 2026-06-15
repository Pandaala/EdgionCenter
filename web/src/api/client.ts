import axios, { AxiosError } from 'axios'
import { message } from 'antd'
import { clearLoggedIn } from '../utils/auth'
import { getActiveControllerId } from '../utils/proxy'

// Create axios instance
export const apiClient = axios.create({
  baseURL: '/api/v1', // forwarded to the Controller Admin API (localhost:12101) via the Vite proxy
  timeout: 30000,
  headers: {
    'Content-Type': 'application/json',
  },
})

// Proxy interceptor — rewrite baseURL when a controller is active (Center proxy mode)
// controller_id contains "/" (e.g. "cluster-east/ctrl-01") which browsers decode
// even when percent-encoded. Use "~" as separator in URL, Center converts back.
apiClient.interceptors.request.use((config) => {
  const controllerId = getActiveControllerId()
  if (controllerId) {
    const safeId = controllerId.replace(/\//g, '~')
    config.baseURL = `/api/v1/proxy/${safeId}/api/v1`
  }
  return config
})

// Request interceptor
apiClient.interceptors.request.use(
  (config) => config,
  (error) => Promise.reject(error)
)

// Response interceptor
apiClient.interceptors.response.use(
  (response) => response,
  (error: AxiosError<{ error?: string }>) => {
    // Handle error responses
    const status = error.response?.status
    let errorMsg: string

    if (status === 401) {
      clearLoggedIn()
      if (window.location.pathname !== '/login') {
        window.location.href = '/login'
      }
      return Promise.reject(error)
    }

    if (status === 409) {
      errorMsg = '资源已存在，无法创建重复资源 / Resource already exists'
    } else if (status === 404) {
      errorMsg = '资源未找到 / Resource not found'
    } else if (status === 400) {
      errorMsg = '请求参数错误 / Bad request'
    } else if (status === 500) {
      errorMsg = '服务器内部错误 / Internal server error'
    } else if (status === 503) {
      errorMsg = '服务暂时不可用 / Service unavailable'
    } else {
      errorMsg = error.response?.data?.error || error.message || '请求失败 / Request failed'
    }
    
    // 仅在非静默模式下弹出错误（Dashboard 的计数查询会静默失败）
    const isSilent = (error.config as any)?._silent
    if (!isSilent) {
      message.error(errorMsg)
    }
    return Promise.reject(error)
  }
)

// 不走 /api/v1 前缀的系统接口，直接用 axios
export const systemClient = axios.create({
  baseURL: '/',
  timeout: 10000,
})

systemClient.interceptors.request.use((config) => {
  const controllerId = getActiveControllerId()
  if (controllerId) {
    const safeId = controllerId.replace(/\//g, '~')
    config.baseURL = `/api/v1/proxy/${safeId}`
  }
  return config
})

export const systemApi = {
  health: async (): Promise<{ success: boolean; data?: string }> => {
    const { data } = await systemClient.get('health')
    return data
  },
  ready: async (): Promise<{ success: boolean; data?: string }> => {
    const { data } = await systemClient.get('ready')
    return data
  },
  serverInfo: async (): Promise<{
    success: boolean
    data?: {
      mode?: string
      server_id?: string
      ready?: boolean
      authzMode?: 'allow_all' | 'rbac'
      dbAuthEnabled?: boolean
    }
  }> => {
    const { data } = await apiClient.get('server-info')
    return data
  },
  reload: async (): Promise<{ success: boolean }> => {
    const { data } = await apiClient.post('reload')
    return data
  },
}

