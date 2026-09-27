import { expect, test as setup } from '@playwright/test'
import { mkdir } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'

setup('kubernetes OIDC authentication', async ({ page, baseURL }) => {
  const username = process.env.E2E_USERNAME
  const password = process.env.E2E_PASSWORD
  if (!username || !password || !baseURL) throw new Error('Kubernetes E2E credentials and base URL are required')
  const callback = new URL(process.env.E2E_OAUTH_CALLBACK_URL ?? 'http://127.0.0.1:14180/oauth2/callback')
  if (callback.hostname !== '127.0.0.1' || callback.protocol !== 'http:' ||
    new URL('/oauth2/callback', baseURL).href !== callback.href) throw new Error('Unexpected OAuth callback URL')
  await page.goto('/')
  await page.getByPlaceholder(/email|username/i).fill(username)
  await page.getByPlaceholder(/password/i).fill(password)
  await page.getByRole('button', { name: /login|sign in/i }).click()
  const grant = page.getByRole('button', { name: /grant access/i })
  if (await grant.isVisible({ timeout: 5_000 }).catch(() => false)) await grant.click()
  await expect(page).not.toHaveURL(/\/dex\/auth/)
  expect((await page.request.get('/api/v1/auth/me')).ok()).toBeTruthy()
  await expect.poll(() => page.evaluate(() => localStorage.getItem('edgion-logged-in'))).toBe('1')
  const state = resolve(process.env.E2E_ARTIFACT_DIR!, 'auth-kubernetes.json')
  await mkdir(dirname(state), { recursive: true })
  await page.context().storageState({ path: state })
})
