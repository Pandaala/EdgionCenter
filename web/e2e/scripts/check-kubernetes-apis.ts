import { spawnSync } from 'node:child_process'
import cleanupKindMap from '../cleanup-kind-map.json'
import inventory from '../fixture-inventory.json'
import { kubectlArgs } from './ledger.ts'

interface DeclaredResource { apiVersion: string; kind: string; scope: 'Cluster' | 'Namespaced' }
interface Discovery { groupVersion?: string; resources?: Array<{ name?: string; kind?: string; namespaced?: boolean }> }

const declared = new Map<string, DeclaredResource>()
for (const item of inventory.catalogResources as DeclaredResource[]) declared.set(item.kind, item)
for (const item of [...inventory.states, ...inventory.supportResources]) {
  if (!declared.has(item.kind)) throw new Error(`Fixture kind ${item.kind} has no catalog API declaration`)
}
declared.set('EdgionController', { apiVersion: 'center.edgion.io/v1alpha1', kind: 'EdgionController', scope: 'Namespaced' })

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
  const mapped = item.kind === 'EdgionController' ? 'edgioncontrollers.center.edgion.io' : cleanupKindMap[item.kind as keyof typeof cleanupKindMap]
  if (!mapped) throw new Error(`No resource mapping for fixture kind ${item.kind}`)
  const resourceName = mapped.split('.', 1)[0]
  const resource = discovery.resources?.find(({ name }) => name === resourceName)
  if (!resource || resource.kind !== item.kind || resource.namespaced !== (item.scope === 'Namespaced')) {
    throw new Error(`Kubernetes discovery mismatch for ${item.apiVersion} ${resourceName}: expected kind=${item.kind} namespaced=${item.scope === 'Namespaced'}`)
  }
}

process.stdout.write(`verified ${declared.size} fixture kinds across ${discoveries.size} served Kubernetes API versions\n`)
