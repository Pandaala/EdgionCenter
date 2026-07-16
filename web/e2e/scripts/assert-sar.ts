import { spawnSync } from 'node:child_process'
import { kubectlArgs } from './ledger.ts'

const [expected, user, verb, group, resource, namespace, name] = process.argv.slice(2)
if (!['yes', 'no'].includes(expected) || !user || !verb || group === undefined || !resource || !namespace || !name) {
  throw new Error('usage: assert-sar.ts yes|no USER VERB GROUP RESOURCE NAMESPACE NAME')
}
const review = {
  apiVersion: 'authorization.k8s.io/v1', kind: 'SubjectAccessReview',
  spec: { user, resourceAttributes: { verb, group, resource, namespace, name } },
}
const result = spawnSync('kubectl', kubectlArgs(['create', '-f', '-', '-o', 'json']), {
  input: JSON.stringify(review), encoding: 'utf8', maxBuffer: 8 * 1024 * 1024,
})
if (result.status !== 0) throw new Error(`SubjectAccessReview failed: ${result.stderr}`)
const body = JSON.parse(result.stdout) as { status?: { allowed?: boolean; reason?: string } }
const actual = body.status?.allowed === true ? 'yes' : 'no'
if (actual !== expected) throw new Error(`SAR assertion failed: expected=${expected} actual=${actual} user=${user} ${verb} ${group}/${resource} ${namespace}/${name} reason=${body.status?.reason ?? ''}`)
