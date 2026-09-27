import { spawnSync } from 'node:child_process'
import { readFileSync, readdirSync } from 'node:fs'
import { resolve } from 'node:path'
import { isDeepStrictEqual } from 'node:util'
import yaml from 'js-yaml'
import cleanupKindMap from '../cleanup-kind-map.json'
import inventory from '../fixture-inventory.json'
import { kubectlArgs } from './ledger.ts'

interface DeclaredResource { apiVersion: string; kind: string; scope: 'Cluster' | 'Namespaced' }
interface Discovery { groupVersion?: string; resources?: Array<{ name?: string; kind?: string; namespaced?: boolean }> }
interface Crd { kind?: string; metadata?: { name?: string }; spec?: { names?: { kind?: string }; versions?: Array<{ name: string; schema?: unknown }> } }

const declared = new Map<string, DeclaredResource>()
for (const item of inventory.catalogResources as DeclaredResource[]) declared.set(item.kind, item)
for (const item of [...inventory.states, ...inventory.supportResources]) {
  if (!declared.has(item.kind)) throw new Error(`Fixture kind ${item.kind} has no catalog API declaration`)
}
const centerResources: Record<string, string> = {
  EdgionController: 'edgioncontrollers.center.edgion.io',
  EdgionProviderAccount: 'edgionprovideraccounts.center.edgion.io',
  EdgionProviderCapabilitySnapshot: 'edgionprovidercapabilitysnapshots.center.edgion.io',
}
for (const kind of Object.keys(centerResources)) {
  declared.set(kind, { apiVersion: 'center.edgion.io/v1alpha1', kind, scope: 'Namespaced' })
}

const discoveries = new Map<string, Discovery>()
for (const item of declared.values()) {
  const [group, version] = item.apiVersion.includes('/') ? item.apiVersion.split('/', 2) : ['', item.apiVersion]
  const endpoint = group ? `/apis/${group}/${version}` : `/api/${version}`
  let discovery = discoveries.get(endpoint)
  if (!discovery) {
    const result = spawnSync('kubectl', kubectlArgs(['get', '--raw', endpoint]), { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 })
    if (result.status !== 0) throw new Error(`Kubernetes API version is not served (${item.apiVersion}): ${result.stderr}`)
    discovery = JSON.parse(result.stdout) as Discovery
    if (discovery.groupVersion !== item.apiVersion) throw new Error(`Discovery groupVersion mismatch for ${endpoint}: ${discovery.groupVersion ?? '<missing>'}`)
    discoveries.set(endpoint, discovery)
  }
  const mapped = centerResources[item.kind] ?? cleanupKindMap[item.kind as keyof typeof cleanupKindMap]
  if (!mapped) throw new Error(`No resource mapping for fixture kind ${item.kind}`)
  const resourceName = mapped.split('.', 1)[0]
  const resource = discovery.resources?.find(({ name }) => name === resourceName)
  if (!resource || resource.kind !== item.kind || resource.namespaced !== (item.scope === 'Namespaced')) {
    throw new Error(`Kubernetes discovery mismatch for ${item.apiVersion} ${resourceName}: expected kind=${item.kind} namespaced=${item.scope === 'Namespaced'}`)
  }
}

// Discovery alone accepts obsolete CRDs that prune current operator fields or
// reject current status envelopes. Compare the custom schemas with the same
// checkout used to build Controllers. Never update shared cluster CRDs here.
const schemaDirectory = resolve(process.env.EDGION_DIR ?? '../../Edgion', 'config/crd/edgion-crd')
const mismatches: string[] = []
let schemas = 0
for (const file of readdirSync(schemaDirectory).filter((name) => name.endsWith('.yaml'))) {
  for (const document of yaml.loadAll(readFileSync(resolve(schemaDirectory, file), 'utf8')) as Crd[]) {
    if (document?.kind !== 'CustomResourceDefinition') continue
    const item = declared.get(document.spec?.names?.kind ?? '')
    if (!item) continue
    const version = item.apiVersion.split('/')[1]
    const expected = document.spec?.versions?.find(({ name }) => name === version)?.schema
    if (!expected || !document.metadata?.name) throw new Error(`Missing source CRD schema for ${item.kind} ${version}`)
    const result = spawnSync('kubectl', kubectlArgs(['get', 'crd', document.metadata.name, '-o', 'json']), { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 })
    if (result.status !== 0) throw new Error(`Unable to inspect CRD ${document.metadata.name}: ${result.stderr}`)
    const live = JSON.parse(result.stdout) as Crd
    const actual = live.spec?.versions?.find(({ name }) => name === version)?.schema
    // Require the checked-in schema exactly, including descriptive metadata;
    // stripping arbitrary keys could hide operator fields with the same name.
    if (!isDeepStrictEqual(expected, actual)) mismatches.push(`${document.metadata.name}/${version}`)
    schemas++
  }
}
const expectedSchemas = [...declared.values()].filter(({ apiVersion }) => apiVersion.startsWith('edgion.io/')).length
if (schemas !== expectedSchemas) throw new Error(`Expected ${expectedSchemas} Edgion CRD schemas, inspected ${schemas}`)
if (mismatches.length) throw new Error(`CRD admission schemas differ from current Edgion: ${mismatches.join(', ')}. Use an isolated cluster with current CRDs; this preflight never overwrites existing schemas.`)
process.stdout.write(`verified ${declared.size} fixture kinds across ${discoveries.size} served Kubernetes API versions and ${schemas} current Edgion CRD schemas\n`)
