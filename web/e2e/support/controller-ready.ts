import { expect, type APIRequestContext } from '@playwright/test'
import {
  controllerAccessPath,
  controllerKindFor,
  parseControllerAccessDocument,
} from '../../src/api/access.ts'
import type {
  ApiResponse,
  ControllerAccessDocument,
  ControllerAccessResourceVerb,
  ResourceKind,
} from '../../src/api/types.ts'

export interface ControllerResourceCapability {
  resourceKind: ResourceKind
  verbs: readonly ControllerAccessResourceVerb[]
}

function readAccessDocument(value: unknown): ControllerAccessDocument {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new Error('response is not an object')
  }
  const response = value as ApiResponse<unknown>
  if (!response.success || response.data === undefined) {
    throw new Error(response.error || 'response is unsuccessful')
  }
  return parseControllerAccessDocument(response.data)
}

function missingCapability(
  document: ControllerAccessDocument,
  capability: ControllerResourceCapability,
): string | undefined {
  const kind = controllerKindFor(capability.resourceKind)
  const row = document.resources.find((resource) => resource.kind === kind)
  if (!row) return `${kind}:missing`
  const missingVerbs = capability.verbs.filter((verb) => !row.verbs.includes(verb))
  return missingVerbs.length > 0 ? `${kind}:${missingVerbs.join(',')}` : undefined
}

/**
 * Poll the real Controller access contract instead of sleeping through a reload.
 * The diagnostic value returned by each probe is retained in Playwright's error.
 */
export async function waitForControllerCapabilities(
  request: APIRequestContext,
  controllerId: string,
  capabilities: readonly ControllerResourceCapability[],
  timeout = 30_000,
): Promise<void> {
  await expect.poll(async () => {
    try {
      const response = await request.get(controllerAccessPath(controllerId), { failOnStatusCode: false })
      if (!response.ok()) return `http:${response.status()}`
      const document = readAccessDocument(await response.json())
      const missing = capabilities.map((capability) => missingCapability(document, capability)).filter(Boolean)
      return missing.length > 0 ? `missing:${missing.join(';')}` : 'ready'
    } catch (error) {
      return `error:${error instanceof Error ? error.message : String(error)}`
    }
  }, {
    message: `Controller ${controllerId} did not expose the required resource capabilities`,
    timeout,
    intervals: [250, 500, 1_000],
  }).toBe('ready')
}
