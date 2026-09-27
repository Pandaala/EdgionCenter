/** Controller DNS server grammar: literal IP, optional socket port, bracketed IPv6. */
export function isValidDnsServer(value: string): boolean {
  if (typeof value !== 'string' || value.trim() !== value || !value) return false
  const ipv4 = (host: string) => host.split('.').length === 4
    && host.split('.').every((part) => /^(0|[1-9][0-9]{0,2})$/.test(part) && Number(part) <= 255)
  const ipv6 = (host: string) => {
    if (!host.includes(':') || !/^[0-9a-f:.]+$/i.test(host)) return false
    try { return new URL(`http://[${host}]/`).hostname.startsWith('[') } catch { return false }
  }
  if (ipv4(value) || ipv6(value)) return true
  const bracketed = /^\[([^\]]+)\](?::([0-9]+))?$/.exec(value)
  if (bracketed) return ipv6(bracketed[1]) && (bracketed[2] === undefined || Number(bracketed[2]) <= 65535)
  const socket = /^([^:]+):([0-9]+)$/.exec(value)
  return !!socket && ipv4(socket[1]) && Number(socket[2]) <= 65535
}
