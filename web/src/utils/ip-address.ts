/** Literal IP syntax shared by DNS endpoints and trusted proxy CIDRs. */
export const isIpv4Literal = (host: string) => !/[^0-9.]/.test(host) && host.split('.').length === 4
  && host.split('.').every((part) => /^(0|[1-9][0-9]{0,2})$/.test(part) && Number(part) <= 255)

export const isIpv6Literal = (host: string) => {
  if (!host.includes(':') || /[^0-9a-f:.]/i.test(host)) return false
  try { return new URL(`http://[${host}]/`).hostname.startsWith('[') } catch { return false }
}

export function isIpOrCidr(value: unknown): boolean {
  if (typeof value !== 'string' || value.trim() !== value) return false
  const parts = value.split('/')
  if (parts.length > 2) return false
  const [address, prefix] = parts
  const maxPrefix = isIpv4Literal(address) ? 32 : isIpv6Literal(address) ? 128 : -1
  return maxPrefix >= 0 && (prefix === undefined || (/^[+]?[0-9]+$/.test(prefix) && Number(prefix) <= maxPrefix))
}
