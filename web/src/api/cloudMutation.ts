import { isAxiosError } from 'axios'

export type CloudMutationErrorResult = 'conflicted' | 'ambiguous' | 'rejected'

/** Classify observations without treating a lost mutation response as rejection. */
export function cloudMutationResult(error: unknown): CloudMutationErrorResult {
  const response = (error as { response?: { status?: number; data?: { error?: string } } } | null)?.response
  if (response?.data?.error === 'unknown_outcome') return 'ambiguous'
  if (response?.status === 409 || response?.status === 412) return 'conflicted'
  if (isAxiosError(error) && !error.response && error.request != null
    && ['POST', 'PUT', 'PATCH', 'DELETE'].includes(error.config?.method?.toUpperCase() ?? '')) {
    return 'ambiguous'
  }
  return 'rejected'
}
