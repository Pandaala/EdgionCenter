import { expect, it } from 'vitest'
import { validateKubeContext } from '../../e2e/support/kube-context'

it('keeps explicit OrbStack access and accepts only the current run kind context', () => {
  expect(validateKubeContext('orbstack', undefined)).toBe('orbstack')
  // SHA-256 of "abc" starts with ba7816bf.
  expect(validateKubeContext('kind-eruie2e-ba7816bf', 'abc')).toBe('kind-eruie2e-ba7816bf')
})

it('rejects missing, unrelated and previous-run contexts', () => {
  for (const context of [undefined, '', 'production', 'kind-kind', 'kind-eruie2e-ba7816bf-extra', 'orbstack ']) {
    expect(() => validateKubeContext(context, 'abc')).toThrow('E2E_KUBE_CONTEXT')
  }
  expect(() => validateKubeContext('kind-eruie2e-ba7816bf', 'different-run')).toThrow()
  expect(() => validateKubeContext('kind-eruie2e-ba7816bf', undefined)).toThrow()
})
