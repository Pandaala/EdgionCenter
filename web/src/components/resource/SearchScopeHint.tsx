import { Alert } from 'antd'
import { useT } from '@/i18n'

interface Props {
  loaded: number
  hasNext: boolean
}

/**
 * Shown between the search input and the Table when:
 *   - The list is paginated and not yet fully loaded (hasNext=true)
 * Tells the user that client-side search only filters items already
 * loaded into memory; deeper matches require loading more pages.
 *
 * Returns null when the full list is in memory (FS/Etcd mode or
 * after the user has paged to the end).
 */
export default function SearchScopeHint({ loaded, hasNext }: Props) {
  const t = useT()
  if (!hasNext) return null
  return (
    <Alert
      type="info"
      showIcon
      banner
      message={t('table.searchLoadedOnly', { n: loaded })}
      style={{ marginBottom: 8 }}
    />
  )
}
