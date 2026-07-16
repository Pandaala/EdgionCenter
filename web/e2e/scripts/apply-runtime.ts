import { spawnSync } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import * as yaml from 'js-yaml'
import inventory from '../fixture-inventory.json'
import { currentContext, kubectlArgs, readCleanupLedger, render, requireRun, resourceForKind, substitutions, writeCleanupLedger, type CleanupLedger, type CleanupObject } from './ledger.ts'

const { runId, artifactDir, prefix } = requireRun(); const context = await currentContext(); const path = resolve(artifactDir, 'cleanup-ledger.json')
let ledger: CleanupLedger
try { ledger = await readCleanupLedger(path) } catch (error) {
  if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
  ledger = { schemaVersion: 1, runId, context, createdAt: new Date().toISOString(), objects: [] }
}
if (ledger.runId !== runId || ledger.context !== context) throw new Error('Runtime apply refused: ledger mismatch')
const extra = {
  __E2E_USERNAME__: process.env.E2E_USERNAME ?? '', __E2E_PASSWORD__: process.env.E2E_PASSWORD ?? '',
  __OAUTH_CLIENT_SECRET__: process.env.E2E_OAUTH_CLIENT_SECRET ?? '',
  __E2E_CONTROLLER_A__: process.env.E2E_CONTROLLER_A ?? '', __E2E_CONTROLLER_B__: process.env.E2E_CONTROLLER_B ?? '',
}
if (Object.values(extra).some((value) => !value)) throw new Error('Runtime credentials are required')
const passwordHashResult = spawnSync('htpasswd', ['-inBC', '10', ''], { input: `${extra.__E2E_PASSWORD__}\n`, encoding: 'utf8' })
if (passwordHashResult.status !== 0) throw new Error(`Unable to hash the Dex test password: ${passwordHashResult.stderr}`)
const passwordHash = passwordHashResult.stdout.trim().replace(/^:/, '').replace(/\$2y\$/, '$2a$')
const oidcIssuer = `https://${prefix}-dex.${prefix}-system.svc.cluster.local:5556/dex`
const dexUserId = `${prefix}-user`
const dexUserIdBytes = Buffer.from(dexUserId)
if (dexUserIdBytes.length > 127) throw new Error('Dex E2E user ID exceeds one-byte protobuf length')
// Dex local-password identities use a protobuf pair { id, connector_id }
// encoded with unpadded base64url as the OIDC `sub` claim.
const dexSubject = Buffer.concat([
  Buffer.from([0x0a, dexUserIdBytes.length]), dexUserIdBytes,
  Buffer.from([0x12, 0x05]), Buffer.from('local'),
]).toString('base64url')
const controllerIdA = `e2e-a/${extra.__E2E_CONTROLLER_A__}`
const controllerIdB = `e2e-b/${extra.__E2E_CONTROLLER_B__}`
const controllerResourceName = (id: string): string => {
  const value = id.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 50).replace(/-+$/g, '') || 'controller'
  return `${value}-${createHash('sha256').update(id).digest('hex').slice(0, 12)}`
}
const source = render(await readFile(resolve('e2e/runtime/kubernetes/runtime.yaml'), 'utf8'), {
  ...substitutions(runId, prefix), ...extra,
  __E2E_PASSWORD_HASH__: passwordHash,
  // Exactly 32 printable bytes. oauth2-proxy accepts the raw value as an AES-256
  // key, while a base64 decoder would also produce the valid AES-192 length.
  __COOKIE_SECRET__: randomBytes(24).toString('base64'),
  __OIDC_SAR_USER__: `oidc:${oidcIssuer.length}:${oidcIssuer}:user:${dexSubject}`,
  __E2E_CONTROLLER_PATH_A__: controllerIdA.replaceAll('/', '~'),
  __E2E_CONTROLLER_PATH_B__: controllerIdB.replaceAll('/', '~'),
  __CONTROLLER_RESOURCE_A__: controllerResourceName(controllerIdA),
  __CONTROLLER_RESOURCE_B__: controllerResourceName(controllerIdB),
})
const documents: unknown[] = []; yaml.loadAll(source, (value) => documents.push(value))
const tlsDir = resolve(artifactDir, 'tls')
const secret = (name: string, stringData: Record<string, string>) => ({ apiVersion: 'v1', kind: 'Secret', metadata: { name: `${prefix}-${name}`, namespace: `${prefix}-system`, labels: { 'edgion.io/e2e-run': runId } }, type: 'Opaque', stringData })
const pem = async (name: string) => readFile(resolve(tlsDir, name), 'utf8')
documents.splice(1, 0,
  secret('center-federation-tls', { 'ca.crt': await pem('ca.crt'), 'server.crt': await pem('server.crt'), 'server.key': await pem('server.key') }),
  secret('center-internal-tls', { 'ca.crt': await pem('internal-ca.crt'), 'tls.crt': await pem('internal.crt'), 'tls.key': await pem('internal.key') }),
  secret('controller-a-federation-tls', { 'ca.crt': await pem('ca.crt'), 'controller.crt': await pem('controller-a.crt'), 'controller.key': await pem('controller-a.key') }),
  secret('controller-b-federation-tls', { 'ca.crt': await pem('ca.crt'), 'controller.crt': await pem('controller-b.crt'), 'controller.key': await pem('controller-b.key') }),
  secret('dex-tls', { 'tls.crt': await pem('dex.crt'), 'tls.key': await pem('dex.key') }),
  secret('center-oidc-ca', { 'ca.crt': await pem('oidc-ca.crt') }),
  secret('oauth-oidc-ca', { 'ca.crt': await pem('oidc-ca.crt') }),
)
const allowedObjects = new Set(inventory.runtimeObjects.map(({ kind, name }) => `${kind}/${prefix}-${name}`))
for (const document of documents as Array<Record<string, any>>) {
  const name = document.metadata?.name
  if (!allowedObjects.has(`${document.kind}/${name}`)) throw new Error(`Runtime object absent from static inventory: ${document.kind}/${name}`)
  if (document.metadata?.labels?.['edgion.io/e2e-run'] !== runId) throw new Error(`Runtime object lacks run label: ${document.kind}/${name}`)
  const result = spawnSync('kubectl', kubectlArgs(['create', '-f', '-', '-o', 'json']), { input: yaml.dump(document), encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 })
  if (result.status !== 0) throw new Error(`Runtime create refused (objects must be absent before the run): ${result.stderr}`)
  const applied = JSON.parse(result.stdout) as Record<string, any>; const scope = applied.metadata.namespace ? 'Namespaced' : 'Cluster'
  const entry: CleanupObject = { apiVersion: applied.apiVersion, kind: applied.kind, resource: resourceForKind(applied.kind), scope, name: applied.metadata.name, runLabel: runId, uid: applied.metadata.uid, phase: applied.kind === 'Secret' ? 'auth' : 'runtime' }
  if (scope === 'Namespaced') entry.namespace = applied.metadata.namespace
  ledger.objects.push(entry); await writeCleanupLedger(path, ledger)
}
process.stdout.write(`applied ${documents.length} runtime objects; ledger=${path}\n`)
