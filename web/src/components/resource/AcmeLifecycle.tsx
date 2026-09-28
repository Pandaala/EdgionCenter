import { Badge, Space, Tooltip, Typography } from 'antd'
import type { ComponentProps } from 'react'
import type { K8sResource } from '@/api/types'
import { useT } from '@/i18n'
import ResourceStatus from './ResourceStatus'

const phaseColors: Record<string, ComponentProps<typeof Badge>['status']> = {
  Ready: 'success', Issuing: 'processing', Renewing: 'processing',
  Pending: 'warning', Failed: 'error',
}

/** Certificate lifecycle observations never become editable configuration. */
export default function AcmeLifecycle({ resource }: { resource: K8sResource }) {
  const t = useT()
  return <ResourceStatus kind="edgionacme" resource={resource} renderStatus={(status) => {
    const phase = typeof status?.phase === 'string' ? status.phase : undefined
    const expiry = typeof status?.certificateNotAfter === 'string' ? status.certificateNotAfter : undefined
    const timestamp = expiry ? Date.parse(expiry) : NaN
    return <Space direction="vertical" size={0} style={{ minWidth: 210 }}>
      {phase ? <Tooltip title={t('acme.lifecycle.hint')}>
        <Badge status={phaseColors[phase] ?? 'default'} text={phase} />
      </Tooltip> : '-'}
      <Typography.Text type="secondary">
        {t('acme.lifecycle.expires')}:<br />{Number.isFinite(timestamp)
          ? <time dateTime={new Date(timestamp).toISOString()}>{new Date(timestamp).toISOString().replace('T', ' ').replace('.000Z', ' UTC')}</time> : '-'}
      </Typography.Text>
    </Space>
  }} />
}
