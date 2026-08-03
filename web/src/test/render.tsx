import type { ReactElement } from 'react'
import { QueryClient, QueryClientProvider, type QueryClientConfig } from '@tanstack/react-query'
import { render } from '@testing-library/react'

export function createTestQueryClient(config?: QueryClientConfig) {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
    ...config,
  })
}

export function renderWithQueryClient(
  element: ReactElement,
  client = createTestQueryClient(),
) {
  return {
    ...render(<QueryClientProvider client={client}>{element}</QueryClientProvider>),
    queryClient: client,
  }
}
