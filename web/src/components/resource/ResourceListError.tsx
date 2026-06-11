import { Result, Button } from 'antd'
import { useT } from '@/i18n'

interface Props {
  error: unknown
  onRetry: () => void
}

interface MaybeApiError {
  response?: {
    status?: number
    data?: { code?: string; error?: string }
  }
  message?: string
}

function describe(err: unknown): { status?: number; code?: string; text: string } {
  const e = err as MaybeApiError
  const status = e?.response?.status
  const code = e?.response?.data?.code
  const text = e?.response?.data?.error ?? e?.message ?? 'Unknown error'
  return { status, code, text }
}

/**
 * Generic error UI shown in place of the Table when the list query
 * fails with anything other than:
 *   - 401: apiClient interceptor already redirects to /login.
 *   - 410 / StalePagination: useResourceList handles silently via toast.
 *
 * Handles 503/NotSupported / 5xx / network failures / unexpected 4xx
 * with a clear message and a retry button.
 */
export default function ResourceListError({ error, onRetry }: Props) {
  const t = useT()
  const { status, code, text } = describe(error)

  let title = t('msg.backendListError')
  if (status === 503 || code === 'NotSupported') {
    title = t('msg.backendListNotSupported')
  }

  return (
    <Result
      status="error"
      title={title}
      subTitle={text}
      extra={
        <Button type="primary" onClick={onRetry}>
          {t('msg.retry')}
        </Button>
      }
    />
  )
}
