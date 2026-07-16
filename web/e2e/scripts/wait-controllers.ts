import { chromium, request as playwrightRequest, type APIResponse, type Page } from '@playwright/test'
import { controllerId } from '../support/controllers.ts'

const mode = process.env.E2E_MODE
const username = process.env.E2E_USERNAME
const password = process.env.E2E_PASSWORD
if (!mode || !username || !password) throw new Error('Controller readiness requires mode and credentials')
const controllerA = controllerId('A')
const controllerB = controllerId('B')

interface ControllerSummary { controller_id?: string; cluster?: string; online?: boolean }
interface ControllerResponse { success?: boolean; data?: ControllerSummary[] }

const expected = new Map([[controllerA, 'e2e-a'], [controllerB, 'e2e-b']])
const deadline = Date.now() + 90_000
const validate = async (response: APIResponse): Promise<string | undefined> => {
  if (!response.ok()) return `HTTP ${response.status()}`
  const body = await response.json() as ControllerResponse
  if (!body.success || !Array.isArray(body.data)) return 'invalid response envelope'
  const actual = new Map(body.data.map((item) => [item.controller_id, item]))
  if (actual.size !== expected.size || [...actual.keys()].some((id) => !id || !expected.has(id))) return `unexpected controller ids: ${[...actual.keys()].join(',')}`
  for (const [id, cluster] of expected) {
    const item = actual.get(id)
    if (item?.cluster !== cluster || item.online !== true) return `${id} is not online in ${cluster}`
  }
  return undefined
}

async function poll(fetchControllers: () => Promise<APIResponse>): Promise<void> {
  let problem = 'not attempted'
  while (Date.now() < deadline) {
    try {
      problem = await validate(await fetchControllers()) ?? ''
      if (!problem) return
    } catch (error) {
      problem = error instanceof Error ? error.message : String(error)
    }
    await new Promise((resolve) => setTimeout(resolve, 1_000))
  }
  throw new Error(`Center controller readiness deadline exceeded: ${problem}`)
}

if (mode === 'standalone') {
  const baseURL = 'http://127.0.0.1:12201'
  let token = ''
  while (Date.now() < deadline && !token) {
    const response = await fetch(`${baseURL}/api/v1/auth/login`, {
      method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ username, password }),
    })
    if (response.ok) {
      const body = await response.json() as { data?: { token?: string } }
      token = body.data?.token ?? ''
    }
    if (!token) await new Promise((resolve) => setTimeout(resolve, 1_000))
  }
  if (!token) throw new Error('Standalone authentication did not become ready')
  const request = await playwrightRequest.newContext({ baseURL, extraHTTPHeaders: { authorization: `Bearer ${token}` } })
  try { await poll(() => request.get('/api/v1/controllers')) } finally { await request.dispose() }
} else if (mode === 'kubernetes') {
  const browser = await chromium.launch({ headless: true })
  const context = await browser.newContext({ baseURL: 'http://127.0.0.1:14180', ignoreHTTPSErrors: true })
  const page: Page = await context.newPage()
  try {
    let navigated = false
    let navigationProblem = 'not attempted'
    while (Date.now() < deadline && !navigated) {
      try {
        await page.goto('/', { waitUntil: 'domcontentloaded' })
        navigated = await page.getByPlaceholder(/email|username/i).isVisible()
        if (!navigated) navigationProblem = `login form absent at ${page.url()}`
      } catch (error) {
        navigationProblem = error instanceof Error ? error.message : String(error)
        await new Promise((resolve) => setTimeout(resolve, 1_000))
      }
    }
    if (!navigated) throw new Error(`Kubernetes login navigation deadline exceeded: ${navigationProblem}`)
    await page.getByPlaceholder(/email|username/i).fill(username)
    await page.getByPlaceholder(/password/i).fill(password)
    await page.getByRole('button', { name: /login|sign in/i }).click()
    const grant = page.getByRole('button', { name: /grant access/i })
    if (await grant.isVisible({ timeout: 5_000 }).catch(() => false)) await grant.click()
    await page.waitForURL((url) => !url.pathname.startsWith('/dex/'), { timeout: 30_000 })
    await poll(() => page.request.get('/api/v1/controllers'))
  } finally { await browser.close() }
} else {
  throw new Error(`Unsupported E2E mode for controller readiness: ${mode}`)
}

process.stdout.write(`Center reports exactly ${controllerA}/e2e-a and ${controllerB}/e2e-b online\n`)
