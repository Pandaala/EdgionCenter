import type { ColumnsType } from 'antd/es/table'
import { Button, DatePicker, Input, Space } from 'antd'
import { SearchOutlined } from '@ant-design/icons'
import type { Dayjs } from 'dayjs'
import type { K8sResource, ResourceKey } from '@/api/types'
import AgeCell from './AgeCell'

interface Options<T extends Row = Row> {
  /** True if the kind is namespaced (adds NAMESPACE column). */
  namespaced: boolean
  /** Translated column titles (caller passes from useT). */
  titles: {
    name: string
    namespace: string
    age: string
  }
  /**
   * Optional dataSource items. If provided, the NAMESPACE column uses a
   * dropdown of the distinct values found in `items` (Cloudflare-style
   * filter); otherwise it falls back to a substring search box.
   */
  items?: T[]
}

type Row = K8sResource | ResourceKey

interface FilterDropdownProps {
  setSelectedKeys: (keys: React.Key[]) => void
  selectedKeys: React.Key[]
  confirm: () => void
  clearFilters?: () => void
}

// Search-box filter dropdown used by NAME / NAMESPACE. Substring match, case-insensitive.
function searchDropdown(placeholder: string) {
  return ({ setSelectedKeys, selectedKeys, confirm, clearFilters }: FilterDropdownProps) => (
    <div style={{ padding: 8 }} onKeyDown={(e) => e.stopPropagation()}>
      <Input
        autoFocus
        placeholder={placeholder}
        value={selectedKeys[0] as string | undefined}
        onChange={(e) => setSelectedKeys(e.target.value ? [e.target.value] : [])}
        onPressEnter={() => confirm()}
        style={{ marginBottom: 8, display: 'block', width: 200 }}
      />
      <Space>
        <Button type="primary" size="small" icon={<SearchOutlined />} onClick={() => confirm()}>Search</Button>
        <Button size="small" onClick={() => { clearFilters?.(); confirm() }}>Reset</Button>
      </Space>
    </div>
  )
}

// Date-range filter dropdown used by AGE (filters on metadata.creationTimestamp).
function dateRangeDropdown({ setSelectedKeys, selectedKeys, confirm, clearFilters }: FilterDropdownProps) {
  // Encode selection as a single string "startMs|endMs" so Antd's filter pipeline accepts it.
  const raw = selectedKeys[0] as string | undefined
  const [start, end] = raw ? raw.split('|').map((s) => (s ? Number(s) : undefined)) : [undefined, undefined]
  return (
    <div style={{ padding: 8 }} onKeyDown={(e) => e.stopPropagation()}>
      <DatePicker.RangePicker
        showTime
        value={start && end ? ([{ valueOf: () => start } as Dayjs, { valueOf: () => end } as Dayjs]) : undefined}
        onChange={(range) => {
          if (range && range[0] && range[1]) {
            setSelectedKeys([`${range[0].valueOf()}|${range[1].valueOf()}`])
          } else {
            setSelectedKeys([])
          }
        }}
        style={{ marginBottom: 8, width: 320 }}
      />
      <Space>
        <Button type="primary" size="small" onClick={() => confirm()}>Apply</Button>
        <Button size="small" onClick={() => { clearFilters?.(); confirm() }}>Reset</Button>
      </Space>
    </div>
  )
}

/**
 * Returns the leading Antd Table columns for any resource list:
 * NAME, NAMESPACE (when namespaced), AGE.
 *
 * All three columns ship with a uniform header-level filter affordance:
 *   - NAME / NAMESPACE: substring search box (matches GIPR / RegionRoute UX)
 *   - AGE: date-range picker over `metadata.creationTimestamp`
 * Plus sortable headers (alpha for name/namespace, chronological for AGE).
 *
 * Spread into the per-page columns array before domain-specific columns:
 *
 *   const columns = [
 *     ...getResourceMetaColumns({ namespaced: true, titles: ... }),
 *     ...domainColumns,
 *     actionColumn,
 *   ]
 */
export function getResourceMetaColumns<T extends Row>(
  opts: Options<T>,
): ColumnsType<T> {
  const filterIcon = (filtered: boolean) => (
    <SearchOutlined style={{ color: filtered ? 'var(--ec-color-brand)' : undefined }} />
  )

  const cols: ColumnsType<T> = [
    {
      title: opts.titles.name,
      dataIndex: ['metadata', 'name'],
      key: 'name',
      sorter: (a: T, b: T) =>
        (a.metadata?.name ?? '').localeCompare(b.metadata?.name ?? ''),
      filterDropdown: searchDropdown('Search name'),
      filterIcon,
      onFilter: (value, record) => {
        const name = (record as T).metadata?.name ?? ''
        return name.toLowerCase().includes(String(value).toLowerCase())
      },
    },
  ]

  if (opts.namespaced) {
    // If caller passes items, use a Cloudflare-style multi-select dropdown
    // of distinct namespaces (NAMESPACE is a small enum in practice).
    // Without items, fall back to a substring search box.
    const distinctNamespaces = opts.items
      ? [...new Set(opts.items.map((i) => i.metadata?.namespace ?? '').filter(Boolean))]
          .sort()
          .map((v) => ({ text: v, value: v }))
      : null

    cols.push({
      title: opts.titles.namespace,
      dataIndex: ['metadata', 'namespace'],
      key: 'namespace',
      sorter: (a: T, b: T) =>
        (a.metadata?.namespace ?? '').localeCompare(b.metadata?.namespace ?? ''),
      ...(distinctNamespaces
        ? {
            filters: distinctNamespaces,
            onFilter: (value, record) => (record as T).metadata?.namespace === value,
          }
        : {
            filterDropdown: searchDropdown('Search namespace'),
            filterIcon,
            onFilter: (value, record) => {
              const ns = (record as T).metadata?.namespace ?? ''
              return ns.toLowerCase().includes(String(value).toLowerCase())
            },
          }),
    })
  }

  cols.push({
    title: opts.titles.age,
    key: 'age',
    width: 140,
    sorter: (a: T, b: T) => {
      const ta = a.metadata?.creationTimestamp ? Date.parse(a.metadata.creationTimestamp) : 0
      const tb = b.metadata?.creationTimestamp ? Date.parse(b.metadata.creationTimestamp) : 0
      return ta - tb
    },
    filterDropdown: dateRangeDropdown,
    onFilter: (value, record) => {
      const [s, e] = String(value).split('|').map(Number)
      if (!s || !e) return true
      const ts = (record as T).metadata?.creationTimestamp
      if (!ts) return false
      const t = Date.parse(ts)
      return t >= s && t <= e
    },
    render: (_: unknown, record: T) => <AgeCell timestamp={record.metadata?.creationTimestamp} />,
  })

  return cols
}
