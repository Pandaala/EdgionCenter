import '@testing-library/jest-dom/vitest'

// jsdom does not implement matchMedia, which Ant Design's responsive components
// touch during render. Provide a no-op stub so component tests can render.
if (typeof window !== 'undefined' && !window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList
}
