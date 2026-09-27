import { isIpv4Literal as ipv4, isIpv6Literal as ipv6 } from './ip-address'

/** Controller DNS server grammar: literal IP, optional socket port, bracketed IPv6. */
export function isValidDnsServer(value: string): boolean {
  if (typeof value !== 'string' || value.trim() !== value || !value) return false
  if (ipv4(value) || ipv6(value)) return true
  const bracketed = /^\[([^\]]+)\](?::([0-9]+))?$/.exec(value)
  if (bracketed) return ipv6(bracketed[1]) && (bracketed[2] === undefined || Number(bracketed[2]) <= 65535)
  const socket = /^([^:]+):([0-9]+)$/.exec(value)
  return !!socket && ipv4(socket[1]) && Number(socket[2]) <= 65535
}
