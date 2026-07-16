import { Card, Descriptions, Empty, Space, Tag } from 'antd'
import ResourceConditions from '@/components/resource/ResourceConditions'
import { useT } from '@/i18n'

interface GatewayStatusDetailsProps {
  status: unknown
}

export default function GatewayStatusDetails({ status }: GatewayStatusDetailsProps) {
  const t = useT()
  const value = status && typeof status === 'object' && !Array.isArray(status) ? status as Record<string, any> : {}
  const listeners = Array.isArray(value.listeners) ? value.listeners : []
  const addresses = Array.isArray(value.addresses) ? value.addresses : []

  if (Object.keys(value).length === 0) return <Empty description={t('status.noConditions')} />

  return <Space direction="vertical" size="middle" style={{ width: '100%' }}>
    <Card title={t('section.statusAddresses')} size="small">
      {addresses.length === 0 ? <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} /> : addresses.map((address: any, index: number) => <Tag key={index}>{address.type ? `${address.type}: ` : ''}{address.value}</Tag>)}
    </Card>
    <ResourceConditions status={value} emptyText={t('status.noConditions')} />
    {listeners.map((listener: any, index: number) => <Card key={`${listener.name}-${index}`} title={listener.name || t('gw.unnamed')} size="small">
      <Descriptions size="small" column={2}>
        <Descriptions.Item label={t('field.attachedRoutes')}>{listener.attachedRoutes ?? 0}</Descriptions.Item>
        <Descriptions.Item label={t('field.supportedKinds')}>{(listener.supportedKinds || []).map((kind: any) => `${kind.group ? `${kind.group}/` : ''}${kind.kind}`).join(', ') || '-'}</Descriptions.Item>
      </Descriptions>
      <ResourceConditions status={listener} emptyText={t('status.noConditions')} compact />
    </Card>)}
  </Space>
}
