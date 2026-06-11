import { Popover, Button, Space } from 'antd'
import { InfoCircleOutlined } from '@ant-design/icons'
import { NODE_TYPE_CONFIG } from './nodes/nodeStyles'
import { useT } from '@/i18n'

export default function TopologyLegend() {
  const t = useT()

  const content = (
    <Space direction="vertical" size={6} style={{ minWidth: 180 }}>
      {Object.entries(NODE_TYPE_CONFIG).map(([key, config]) => (
        <div key={key} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <div
            style={{
              width: 14,
              height: 14,
              borderRadius: 3,
              background: config.bgColor,
              border: `2px solid ${config.color}`,
              flexShrink: 0,
            }}
          />
          <span style={{ fontSize: 13 }}>{t(config.label)}</span>
        </div>
      ))}
    </Space>
  )

  return (
    <Popover
      content={content}
      title={t('topology.legend')}
      trigger="click"
      placement="bottomRight"
    >
      <Button icon={<InfoCircleOutlined />}>{t('topology.legend')}</Button>
    </Popover>
  )
}
