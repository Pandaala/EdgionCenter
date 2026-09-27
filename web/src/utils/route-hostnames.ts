import { HOSTNAME_MAX_LENGTH, HOSTNAME_PATTERN } from '@/constants/gateway-api'

/** Mirrors Edgion's validate_route_hostnames and RadixHostnamePatternRef parser. */
export function validateRouteHostnames(hostnames: unknown, kind: 'HTTPRoute' | 'GRPCRoute' | 'TLSRoute'): void {
  const required = kind === 'TLSRoute'
  const names = hostnames ?? []
  if (!Array.isArray(names)) throw new Error('spec.hostnames must be an array')
  if (required && names.length === 0) throw new Error('TLSRoute requires at least one hostname in spec.hostnames')
  const limit = required ? 1024 : 16
  if (names.length > limit) throw new Error(`spec.hostnames must contain at most ${limit} entries for ${kind}`)
  for (const [index, name] of names.entries()) {
    const path = `spec.hostnames[${index}]`
    if (typeof name !== 'string' || !name.length || name.length > HOSTNAME_MAX_LENGTH || HOSTNAME_PATTERN.exec(name)?.[0] !== name) {
      throw new Error(`${path} must contain 1-253 lowercase DNS characters with only an optional leading '*.'`)
    }
    // IPv6 is already excluded by the DNS grammar. Match Rust's strict IPv4
    // parser: wildcard suffixes and zero-padded numeric DNS labels are not IPs.
    const parts = name.split('.')
    if (parts.length === 4 && parts.every((part) => /^(0|[1-9][0-9]{0,2})$/.test(part) && Number(part) <= 255)) {
      throw new Error(`${path} must not be an IP address literal`)
    }
  }
}
