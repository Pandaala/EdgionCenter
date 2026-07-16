import type { CenterCapabilities } from '@/api/client'

export interface ServerDiscoveryResponse {
  success: boolean
  data?: {
    mode?: string
    capabilities?: CenterCapabilities
  }
}

export function resolveServerDiscovery(response: ServerDiscoveryResponse): {
  mode: 'center' | 'controller'
  capabilities: CenterCapabilities | null
} {
  const mode = response.data?.mode
  if (!response.success || (mode !== 'center' && mode !== 'controller')) {
    throw new Error('invalid server discovery response')
  }
  if (mode === 'center' && !response.data?.capabilities) {
    throw new Error('center capability discovery is missing')
  }
  return { mode, capabilities: response.data?.capabilities ?? null }
}
