import { defineConfig, devices } from '@playwright/test'
import { resolve } from 'node:path'

const mode = process.env.E2E_MODE ?? 'standalone'
if (!['standalone', 'kubernetes', 'mock'].includes(mode)) throw new Error(`Unsupported E2E_MODE: ${mode}`)
const artifactRoot = resolve(process.env.E2E_ARTIFACT_DIR ?? `test-results/${mode}`)
const authState = resolve(artifactRoot, `auth-${mode}.json`)

export default defineConfig({
  globalSetup: './e2e/global-setup.ts', testDir: './e2e', outputDir: resolve(artifactRoot, 'playwright'),
  timeout: 30_000, expect: { timeout: 8_000 }, fullyParallel: false,
  // Runtime cases share and deliberately reload the same two Controllers.
  // Keep their destructive workflows ordered; mock-static has no shared runtime.
  workers: mode === 'mock' ? undefined : 1,
  forbidOnly: Boolean(process.env.CI), retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { outputFolder: resolve(artifactRoot, 'html'), open: 'never' }], ['./e2e/reporters/case-ledger-reporter.ts']],
  use: {
    baseURL: process.env.E2E_BASE_URL ?? (mode === 'kubernetes' ? 'http://127.0.0.1:14180' : 'http://127.0.0.1:15173'),
    ignoreHTTPSErrors: mode === 'kubernetes',
    trace: 'retain-on-failure', screenshot: 'only-on-failure', video: 'retain-on-failure',
  },
  projects: mode === 'mock' ? [{ name: 'mock-static', testMatch: /specs\/mock-static\.spec\.ts/, use: { ...devices['Desktop Chrome'] } }] : [
    { name: `auth-${mode}`, testMatch: new RegExp(`auth/${mode}\\.setup\\.ts`) },
    { name: mode, testMatch: /specs\/(?!mock-static).*\.spec\.ts/, dependencies: [`auth-${mode}`], use: { ...devices['Desktop Chrome'], storageState: authState } },
  ],
})
