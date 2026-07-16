import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { requireRun, render } from './ledger.ts'

const [sourcePath, outputPath] = process.argv.slice(2)
if (!sourcePath || !outputPath) throw new Error('usage: render-runtime.ts SOURCE OUTPUT')
const { artifactDir } = requireRun(); const tlsDir = resolve(artifactDir, 'tls')
const required = ['E2E_USERNAME', 'E2E_PASSWORD', 'E2E_CONTROLLER_A', 'E2E_CONTROLLER_B'] as const
for (const key of required) if (!process.env[key]) throw new Error(`${key} is required`)
const values = {
  __ARTIFACT_DIR__: artifactDir, __TLS_DIR__: tlsDir, __E2E_USERNAME__: process.env.E2E_USERNAME!, __E2E_PASSWORD__: process.env.E2E_PASSWORD!,
  __E2E_CONTROLLER_A__: process.env.E2E_CONTROLLER_A!, __E2E_CONTROLLER_B__: process.env.E2E_CONTROLLER_B!,
  __JWT_SECRET__: process.env.E2E_JWT_SECRET ?? 'e2e-runtime-secret-must-not-be-reused',
}
await mkdir(dirname(resolve(outputPath)), { recursive: true }); await writeFile(resolve(outputPath), render(await readFile(resolve(sourcePath), 'utf8'), values), { mode: 0o600 })
