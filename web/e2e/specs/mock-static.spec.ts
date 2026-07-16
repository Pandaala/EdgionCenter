import { expect, test } from '@playwright/test'
import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { resolveRunId } from '../global-setup'
import { generateCases } from '../scripts/generate-cases'

test('static inventory expands two modes and two controllers', async () => {
  const cases = await generateCases()
  expect(cases.length).toBeGreaterThan(100)
  expect(new Set(cases.map(({ mode }) => mode))).toEqual(new Set(['standalone', 'kubernetes']))
  expect(new Set(cases.map(({ controller }) => controller))).toEqual(new Set(['A', 'B']))
})
test('run IDs are unique and label-safe', () => {
  const value = resolveRunId(new Date('2026-07-15T00:00:00.000Z'), 42)
  expect(value).toBe('resource-ui-20260715-000000000Z-42')
  expect(value).toMatch(/^[A-Za-z0-9-]+$/)
})
test('cleanup map contains no broad resource aliases', async () => {
  const map = JSON.parse(await readFile(resolve('e2e/cleanup-kind-map.json'), 'utf8')) as Record<string, string>
  expect(Object.keys(map)).toHaveLength(21)
  expect(Object.values(map)).not.toContain('all')
  expect(Object.values(map).every((value) => !value.includes('*'))).toBeTruthy()
})
