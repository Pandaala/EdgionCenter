import { describe, expect, it } from 'vitest'
import { isIpOrCidr } from './ip-address'

describe('trusted proxy IP/CIDR syntax', () => {
  it('accepts literal addresses, host bits and bounded prefixes', () => {
    for (const value of ['192.0.2.1', '192.0.2.1/24', '0.0.0.0/0', '::', '2001:db8::1/64', '::ffff:192.0.2.1/128', '::/0', '10.0.0.1/+08']) {
      expect(isIpOrCidr(value), value).toBe(true)
    }
  })
  it('rejects malformed addresses, ports, extra separators and out-of-range prefixes', () => {
    for (const value of ['not:an:ip', '1:2:3', ':::', '10.0.0.0/8/extra', '::/64/extra', '010.0.0.1', '192.0.2.1:80', '[::1]', 'fe80::1%en0', '::/129', '10.0.0.1/33', '::/-1', '::/', '::/1.5', ' ::1', '::1\n', '192.0.2.1\n', '10.0.0.1/8\n', '', null, 42]) {
      expect(isIpOrCidr(value), String(value)).toBe(false)
    }
  })
})
