import { readFile, readdir } from 'node:fs/promises'
import { dirname, extname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import yaml from 'js-yaml'
import { RESOURCE_CATALOG } from '../../src/config/resourceCatalog.ts'
import { generateCases } from './generate-cases.ts'
import { buildCasReplacement } from '../support/reset-document.ts'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const e2e = resolve(root, 'e2e')
async function json<T>(path: string): Promise<T> { return JSON.parse(await readFile(path, 'utf8')) as T }
async function files(path: string): Promise<string[]> {
  const result: string[] = []
  for (const entry of await readdir(path, { withFileTypes: true })) {
    const child = resolve(path, entry.name); if (entry.isDirectory()) result.push(...await files(child)); else result.push(child)
  }
  return result
}

const cleanupMap = await json<Record<string, string>>(resolve(e2e, 'cleanup-kind-map.json'))
const fixture = await json<{ namespaces: string[]; catalogResources: Array<{ kind: string; file: string }>; states: Array<{ state: string; file: string }>; supportResources: Array<{ kind: string; file: string }>; runtimeKinds: string[]; runtimeIdentities: string[]; runtimeObjects: Array<{ kind: string; name: string }> }>(resolve(e2e, 'fixture-inventory.json'))
const action = await json<{ global: string[]; auxiliary: string[]; auxiliaryActions: string[]; pages: Record<string, string[]>; resourceActionTemplates: string[]; searchableActionTemplates: string[]; namespacedActionTemplates: string[]; mutableActionTemplates: string[]; batchActionTemplates: string[]; batchKinds: string[]; searchExceptions: string[]; restrictedDependencyTemplates: string[]; resourceSpecific: Record<string, string[]> }>(resolve(e2e, 'action-inventory.json'))
for (const schema of ['case-ledger.schema.json', 'cleanup-ledger.schema.json']) {
  const value = await json<{ $schema?: string; required?: string[]; $defs?: unknown }>(resolve(e2e, schema))
  if (!value.$schema || !value.required?.length || !value.$defs) throw new Error(`Incomplete JSON schema: ${schema}`)
}
const resetProbe = buildCasReplacement(
  { apiVersion: 'v1', kind: 'Service', metadata: { name: 'fixture' }, spec: { ports: [{ port: 80 }] } },
  { apiVersion: 'v1', kind: 'Service', metadata: { name: 'fixture', resourceVersion: '7' }, spec: { clusterIP: '10.96.0.8', clusterIPs: ['10.96.0.8'], ipFamilies: ['IPv4'], ipFamilyPolicy: 'SingleStack', ports: [{ port: 81 }], selector: { drift: 'must-be-removed' } } },
)
if (resetProbe.metadata?.resourceVersion !== '7' || resetProbe.spec?.clusterIP !== '10.96.0.8' || resetProbe.spec?.selector !== undefined) throw new Error('Service CAS reset must preserve immutable allocation and remove mutable drift')
const catalog = [...RESOURCE_CATALOG.values()]
if (catalog.length !== 21) throw new Error(`Expected 21 catalog kinds, received ${catalog.length}`)
const catalogKinds = new Set(catalog.map(({ displayName }) => displayName))
const fixtureKinds = new Set(fixture.catalogResources.map(({ kind }) => kind))
for (const kind of catalogKinds) if (!fixtureKinds.has(kind)) throw new Error(`Missing fixture for ${kind}`)
for (const kind of fixtureKinds) if (!catalogKinds.has(kind)) throw new Error(`Fixture kind absent from catalog: ${kind}`)
if (Object.keys(cleanupMap).length !== 21) throw new Error('Cleanup map must contain exactly 21 catalog kinds')
for (const kind of catalogKinds) if (!cleanupMap[kind]) throw new Error(`Cleanup map missing ${kind}`)
const states = new Set(fixture.states.map(({ state }) => state))
for (const state of ['accepted', 'rejected', 'unresolved', 'referencegrant-denied', 'referencegrant-allowed', 'conflict']) if (!states.has(state)) throw new Error(`Missing state fixture: ${state}`)
if (fixture.namespaces.length < 3) throw new Error('At least three task namespaces are required')
for (const identity of ['controller-a', 'controller-b']) if (!fixture.runtimeIdentities.includes(identity)) throw new Error(`Missing runtime identity: ${identity}`)
const declaredRuntimeIdentities = new Set(fixture.runtimeIdentities)
const manifestRuntimeIdentities = new Set(fixture.runtimeObjects.filter(({ kind }) => kind !== 'Namespace').map(({ name }) => name))
for (const identity of declaredRuntimeIdentities) if (!manifestRuntimeIdentities.has(identity)) throw new Error(`Runtime identity absent from runtimeObjects: ${identity}`)
for (const identity of manifestRuntimeIdentities) if (!declaredRuntimeIdentities.has(identity)) throw new Error(`runtimeObjects identity absent from runtimeIdentities: ${identity}`)
for (const kind of ['Namespace', 'Deployment', 'Service', 'ServiceAccount', 'Role', 'RoleBinding', 'ClusterRole', 'ClusterRoleBinding', 'ConfigMap', 'Secret']) if (!fixture.runtimeKinds.includes(kind)) throw new Error(`Missing runtime kind: ${kind}`)
for (const item of fixture.catalogResources) await readFile(resolve(e2e, 'fixtures', item.file), 'utf8')
for (const item of fixture.states) await readFile(resolve(e2e, 'fixtures', item.file), 'utf8')
for (const item of fixture.supportResources) await readFile(resolve(e2e, 'fixtures', item.file), 'utf8')
const expand = (template: string, kind: string) => template.replace('{kind}', kind)
const declared = new Set([...action.global, ...action.auxiliary, ...Object.values(action.pages).flat()])
for (const item of catalog) {
  if (item.lifecycle === 'restrictedDependency') {
    action.restrictedDependencyTemplates.forEach((value) => declared.add(expand(value, item.kind)))
  } else {
    action.resourceActionTemplates.forEach((value) => declared.add(expand(value, item.kind)))
    if (!action.searchExceptions.includes(item.kind)) action.searchableActionTemplates.forEach((value) => declared.add(expand(value, item.kind)))
    if (item.scope === 'namespaced') action.namespacedActionTemplates.forEach((value) => declared.add(expand(value, item.kind)))
    action.mutableActionTemplates.forEach((value) => declared.add(expand(value, item.kind)))
    if (action.batchKinds.includes(item.kind)) action.batchActionTemplates.forEach((value) => declared.add(expand(value, item.kind)))
  }
  action.resourceSpecific[item.kind]?.forEach((value) => declared.add(value))
}
const sourceFiles = (await files(resolve(root, 'src'))).filter((path) => ['.ts', '.tsx'].includes(extname(path)) && !/\.test\./.test(path))
const implemented = new Set<string>()
for (const path of sourceFiles) {
  const source = await readFile(path, 'utf8')
  for (const match of source.matchAll(/data-testid\s*=\s*["']([^"']+)["']/g)) implemented.add(match[1])
  for (const match of source.matchAll(/["']data-testid["']\s*:\s*["']([^"']+)["']/g)) implemented.add(match[1])
  for (const match of source.matchAll(/resourceActionTestId\(["']([^"']+)["']\s*,\s*["']([^"']+)["']\)/g)) implemented.add(`${match[1]}-${match[2]}`)
  const resourceKindConstant = source.match(/const RESOURCE_KIND\s*=\s*["']([^"']+)["']/)?.[1]
  if (resourceKindConstant) for (const match of source.matchAll(/resourceActionTestId\(RESOURCE_KIND\s*,\s*["']([^"']+)["']\)/g)) implemented.add(`${resourceKindConstant}-${match[1]}`)
  for (const match of source.matchAll(/resourceActionTestId\(kind\s*,\s*["']([^"']+)["']\)/g)) for (const restrictedKind of ['secret', 'configmap']) implemented.add(`${restrictedKind}-${match[1]}`)
  for (const match of source.matchAll(/<PermissionAwareButton\b[\s\S]*?>/g)) {
    const tag = match[0]
    const kind = tag.match(/resourceKind=["']([^"']+)["']/)?.[1]
    const verb = tag.match(/resourceVerb=["']([^"']+)["']/)?.[1]
    const actionName = verb === 'list' || verb === 'list-keys' ? 'refresh' : verb === 'get' ? 'row-view' : verb === 'create' ? 'create' : verb === 'update' ? 'row-edit' : verb === 'delete' ? 'row-delete' : undefined
    if (kind && actionName) implemented.add(`${kind}-${actionName}`)
  }
}
const undocumented = [...implemented].filter((id) => !declared.has(id))
if (undocumented.length) throw new Error(`Undocumented data-testid values: ${undocumented.join(', ')}`)
if (process.env.E2E_INVENTORY_STRICT === '1') {
  const missing = [...declared].filter((id) => !implemented.has(id) && !id.startsWith('count-'))
  if (missing.length) throw new Error(`Unimplemented inventory actions: ${missing.join(', ')}`)

  const resourceWorkflow = await readFile(resolve(e2e, 'specs/resource-actions.spec.ts'), 'utf8')
  const mutationWorkflow = await readFile(resolve(e2e, 'specs/resource-mutations.spec.ts'), 'utf8')
  if (!resourceWorkflow?.includes("`${mode}-A-action-${catalog.kind}`")) throw new Error('Per-resource action workflow ledger annotation is absent')
  for (const token of ["method() === 'POST'", "method() === 'PUT'", "method() === 'DELETE'", 'expectApiDocument', 'expectApiAbsent', "`${mode}-A-crud-${catalog.kind}`"]) {
    if (!mutationWorkflow?.includes(token)) throw new Error(`Per-resource mutation workflow is missing proof step: ${token}`)
  }
  const runtimeCases = await generateCases()
  for (const mode of ['standalone', 'kubernetes']) for (const item of catalog) {
    for (const workflow of ['action', 'crud']) {
      const id = `${mode}-A-${workflow}-${item.kind}`
      if (!runtimeCases.some((item) => item.id === id)) throw new Error(`Runtime workflow case is absent: ${id}`)
    }
  }
  for (const token of ['installActionRecorder(page)', 'expectActionsExercised(page', '(formControls[catalog.kind] ?? [])']) {
    if (!resourceWorkflow?.includes(token)) throw new Error(`Runtime action evidence is missing: ${token}`)
  }
  for (const kind of ['tcproute', 'udproute', 'tlsroute']) {
    if (!resourceWorkflow?.includes(`${kind}: ['streamroute-rule-add', 'streamroute-rule-remove']`)) throw new Error(`Shared StreamRoute controls are not exercised for ${kind}`)
  }
  for (const id of ['metadata-annotation-add', 'metadata-annotation-key', 'metadata-annotation-value', 'editor-form-tab', 'editor-yaml-tab', 'editor-submit']) {
    if (!mutationWorkflow?.includes(`'${id}'`)) throw new Error(`All-resource editor round-trip is missing interaction: ${id}`)
  }
  if (!mutationWorkflow?.includes('for (const catalog of RESOURCE_CATALOG.values())')) throw new Error('Editor round-trip must execute for every catalog kind')
  if (!mutationWorkflow?.includes("page.locator('.ant-tabs-tab[data-node-key=\"conditions\"]')")) throw new Error('All-resource Conditions tab workflow is absent')
  if (!mutationWorkflow?.includes("['edgion.io/e2e-concurrent-writer']")) throw new Error('ACME stale-write preservation oracle is absent')
}
const cases = await generateCases()
const runtimeManifest = await readFile(resolve(e2e, 'runtime/kubernetes/runtime.yaml'), 'utf8')
for (const controller of ['controller-a', 'controller-b']) {
  if (!runtimeManifest.includes(`__PREFIX__-${controller}`)) throw new Error(`Kubernetes runtime missing ${controller}`)
}
const runtimeDocuments: Array<{ kind?: string; metadata?: { name?: string } }> = []
yaml.loadAll(runtimeManifest, (document) => { if (document) runtimeDocuments.push(document as typeof runtimeDocuments[number]) })
type DeploymentDocument = { kind?: string; metadata?: { name?: string }; spec?: { strategy?: { rollingUpdate?: { maxSurge?: number; maxUnavailable?: number } }; template?: { spec?: { containers?: Array<{ name?: string; command?: string[]; args?: string[] }> } } } }
for (const controller of ['controller-a', 'controller-b']) {
  const deployment = (runtimeDocuments as DeploymentDocument[]).find(({ kind, metadata }) => kind === 'Deployment' && metadata?.name === `__PREFIX__-${controller}`)
  const container = deployment?.spec?.template?.spec?.containers?.find(({ name }) => name === 'controller')
  if (container?.command?.join(' ') !== '/usr/local/bin/edgion-controller') throw new Error(`Kubernetes ${controller} must override the shell image entrypoint`)
  if (container.args?.join(' ') !== `--config-file /config/${controller}.yaml`) throw new Error(`Kubernetes ${controller} must use its matching runtime config`)
  if (deployment?.spec?.strategy?.rollingUpdate?.maxSurge !== 0 || deployment?.spec?.strategy?.rollingUpdate?.maxUnavailable !== 1) {
    throw new Error(`Kubernetes ${controller} rollout must release the leader-only lease before replacement readiness`)
  }
}
type RbacDocument = { kind?: string; metadata?: { name?: string; namespace?: string }; rules?: Array<{ apiGroups?: string[]; resources?: string[]; verbs?: string[]; nonResourceURLs?: string[] }>; roleRef?: { name?: string }; subjects?: Array<{ name?: string }> }
const rbacDocuments = runtimeDocuments as RbacDocument[]
for (const document of rbacDocuments.filter(({ kind }) => kind === 'ClusterRole' || kind === 'Role')) {
  for (const rule of document.rules ?? []) {
    if ([...(rule.apiGroups ?? []), ...(rule.resources ?? []), ...(rule.verbs ?? []), ...(rule.nonResourceURLs ?? [])].includes('*')) {
      throw new Error(`Wildcard Kubernetes RBAC is forbidden: ${document.metadata?.name}`)
    }
  }
}
const clusterRole = rbacDocuments.find(({ kind, metadata }) => kind === 'ClusterRole' && metadata?.name === '__PREFIX__-controller-cluster-role')
const actualClusterResources = new Set((clusterRole?.rules ?? []).flatMap(({ resources }) => resources ?? []))
const expectedClusterResources = new Set([
  'gatewayclasses', 'gatewayclasses/status', 'edgiongatewayclassconfigs', 'edgiongatewayclassconfigs/status', 'namespaces',
])
if (actualClusterResources.size !== expectedClusterResources.size || [...actualClusterResources].some((resource) => !expectedClusterResources.has(resource))) throw new Error('Controller cluster RBAC exceeds the exact cluster-scoped resource set')
const globalMutationResources = (clusterRole?.rules ?? [])
  .filter(({ verbs }) => verbs?.some((verb) => ['create', 'update', 'patch', 'delete'].includes(verb)))
  .flatMap(({ resources }) => resources ?? [])
const expectedGlobalMutationResources = new Set(['gatewayclasses', 'gatewayclasses/status', 'edgiongatewayclassconfigs', 'edgiongatewayclassconfigs/status'])
if (globalMutationResources.length !== expectedGlobalMutationResources.size || globalMutationResources.some((resource) => !expectedGlobalMutationResources.has(resource))) throw new Error('Controller global mutation RBAC exceeds cluster-scoped resources')
const controllerNamespaceBindings = rbacDocuments.filter(({ kind, roleRef }) => kind === 'RoleBinding' && roleRef?.name === '__PREFIX__-controller-namespaced-role')
const allowedBindingNamespaces = new Set(['__NS_A__', '__NS_B__', '__NS_DENIED__'])
if (controllerNamespaceBindings.length !== 6 || controllerNamespaceBindings.some(({ metadata }) => !metadata?.namespace || !allowedBindingNamespaces.has(metadata.namespace))) throw new Error('Controller namespaced RBAC must have exactly two bindings in each run namespace')
for (const binding of rbacDocuments.filter(({ kind, subjects }) => kind === 'ClusterRoleBinding' && subjects?.some(({ name }) => name?.includes('controller-')))) {
  if (binding.roleRef?.name !== '__PREFIX__-controller-cluster-role') throw new Error(`Controller service account is cluster-bound to an unexpected role: ${binding.metadata?.name}`)
}
const userRole = rbacDocuments.find(({ kind, metadata }) => kind === 'ClusterRole' && metadata?.name === '__PREFIX__-e2e-user-role')
const userNonResourceUrls = (userRole?.rules ?? []).flatMap(({ nonResourceURLs }) => nonResourceURLs ?? [])
if (userNonResourceUrls.some((url) => url.startsWith('/api/'))) throw new Error('Center authorization must not overlap Kubernetes system:discovery /api/* grants')
const permissionUrls = userNonResourceUrls.filter((url) => url.startsWith('/edgion-center-authz/permissions/'))
if (permissionUrls.some((url) => url.includes('*'))) throw new Error('OIDC permission discovery URLs must never contain wildcards')
const expectedUserNonResourceUrls = new Set([
  '/edgion-center-authz/api/v1/server-info',
  '/edgion-center-authz/api/v1/proxy/__E2E_CONTROLLER_PATH_A__/*',
  '/edgion-center-authz/api/v1/proxy/__E2E_CONTROLLER_PATH_B__/*',
  '/edgion-center-authz/api/v1/center/region-routes',
  '/edgion-center-authz/api/v1/center/region-routes/*',
  '/edgion-center-authz/api/v1/center/cluster-region-routes',
  '/edgion-center-authz/api/v1/center/cluster-region-routes/*',
  '/edgion-center-authz/api/v1/center/service-region-routes',
  '/edgion-center-authz/api/v1/center/service-region-routes/*',
  '/edgion-center-authz/api/v1/center/global-connection-ip-restrictions',
  '/edgion-center-authz/api/v1/center/global-connection-ip-restrictions/*',
  '/edgion-center-authz/api/v1/center/admin/watch-status',
  '/edgion-center-authz/api/v1/center/admin/metadata-store',
  '/edgion-center-authz/permissions/controllers:read',
  '/edgion-center-authz/permissions/controllers:write',
  '/edgion-center-authz/permissions/region-routes:read',
  '/edgion-center-authz/permissions/region-routes:write',
  '/edgion-center-authz/permissions/ip-restrictions:read',
  '/edgion-center-authz/permissions/ip-restrictions:write',
  '/edgion-center-authz/permissions/server:read',
  '/edgion-center-authz/permissions/proxy:access',
])
if (userNonResourceUrls.length !== expectedUserNonResourceUrls.size || userNonResourceUrls.some((url) => !expectedUserNonResourceUrls.has(url))) throw new Error('OIDC non-resource RBAC must equal the reviewed least-privilege URL set')
const dynamicSecrets = ['center-federation-tls', 'center-internal-tls', 'controller-a-federation-tls', 'controller-b-federation-tls', 'dex-tls', 'center-oidc-ca', 'oauth-oidc-ca']
const runtimeTemplateName = (name?: string) => name
  ?.replace('__NS_A__', 'a')
  .replace('__NS_B__', 'b')
  .replace('__NS_DENIED__', 'denied')
  .replace('__PREFIX__-', '')
const manifestObjects = runtimeDocuments.map(({ kind, metadata }) => ({ kind, name: runtimeTemplateName(metadata?.name) }))
const actualRuntime = new Set([...manifestObjects, ...dynamicSecrets.map((name) => ({ kind: 'Secret', name }))].map(({ kind, name }) => {
  if (!kind || !name) throw new Error('Kubernetes runtime document is missing kind or metadata.name')
  return `${kind}/${name}`
}))
const expectedRuntime = new Set(fixture.runtimeObjects.map(({ kind, name }) => `${kind}/${name}`))
const missingRuntime = [...expectedRuntime].filter((identity) => !actualRuntime.has(identity))
const extraRuntime = [...actualRuntime].filter((identity) => !expectedRuntime.has(identity))
if (actualRuntime.size !== runtimeDocuments.length + dynamicSecrets.length) throw new Error('Kubernetes runtime contains duplicate object identities')
if (missingRuntime.length || extraRuntime.length) throw new Error(`Runtime inventory mismatch; missing=${missingRuntime.join(',')} extra=${extraRuntime.join(',')}`)
for (const script of ['run.sh', 'seed.sh', 'reset.sh', 'cleanup.sh']) await readFile(resolve(e2e, 'scripts', script), 'utf8')
process.stdout.write(`inventory ok: ${catalog.length} kinds, ${states.size} states, ${declared.size} actions, ${cases.length} cases, ${implemented.size} implemented selectors${process.env.E2E_INVENTORY_STRICT === '1' ? ' (strict)' : ''}\n`)
