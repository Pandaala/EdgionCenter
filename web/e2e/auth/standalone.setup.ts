import { expect, test as setup } from '@playwright/test'
import { mkdir } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'

setup('standalone password authentication', async ({ page }) => {
  const username = process.env.E2E_USERNAME
  const password = process.env.E2E_PASSWORD
  if (!username || !password) throw new Error('E2E_USERNAME and E2E_PASSWORD are required')
  await page.goto('/login')
  await page.getByTestId('login-username').fill(username)
  await page.getByTestId('login-password').fill(password)
  await page.getByTestId('login-submit').click()
  await expect(page).not.toHaveURL(/\/login/)
  const state = resolve(process.env.E2E_ARTIFACT_DIR!, 'auth-standalone.json')
  await mkdir(dirname(state), { recursive: true })
  await page.context().storageState({ path: state })
})
