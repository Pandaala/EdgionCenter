import { Drawer, Typography, Tag, Divider } from 'antd'
import yaml from 'js-yaml'

const { Text } = Typography

interface Props {
  visible: boolean
  data: {
    kind: string
    name: string
    namespace?: string
    resource: any
    _matchedListener?: string
    [key: string]: any
  } | null
  onClose: () => void
}

function GatewayListeners({ resource, matchedListener }: { resource: any; matchedListener?: string }) {
  const listeners: any[] = resource?.spec?.listeners ?? []
  if (listeners.length === 0) return null

  return (
    <>
      <Divider style={{ margin: '12px 0 8px' }}>Listeners</Divider>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6, marginBottom: 12 }}>
        {listeners.map((l: any, i: number) => {
          const matched = l.name === matchedListener
          return (
            <div
              key={i}
              style={{
                padding: '6px 10px',
                border: `1px solid ${matched ? '#52c41a' : '#f0f0f0'}`,
                borderLeft: `3px solid ${matched ? '#52c41a' : '#d9d9d9'}`,
                borderRadius: 4,
                background: matched ? '#f6ffed' : '#fafafa',
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                flexWrap: 'wrap',
              }}
            >
              <Text strong style={{ color: matched ? '#389e0d' : '#262626', fontSize: 13 }}>
                {l.name}
              </Text>
              {l.port && <Tag style={{ margin: 0 }}>:{l.port}</Tag>}
              {l.protocol && <Tag style={{ margin: 0 }} color="blue">{l.protocol}</Tag>}
              {l.hostname && <Text type="secondary" style={{ fontSize: 11 }}>{l.hostname}</Text>}
              {matched && <Tag color="success" style={{ margin: 0 }}>matched</Tag>}
            </div>
          )
        })}
      </div>
    </>
  )
}

export default function TopologyDetailDrawer({ visible, data, onClose }: Props) {
  const rawYaml = data?.resource ? yaml.dump(data.resource, { indent: 2, lineWidth: -1 }) : ''
  const isGateway = data?.kind === 'gateway'

  return (
    <Drawer
      title={
        data && (
          <div style={{ lineHeight: 1.6 }}>
            <div>
              <Tag color="blue" style={{ marginRight: 4 }}>{data.resource?.apiVersion ?? data.kind}</Tag>
              <Tag>{data.resource?.kind ?? data.kind}</Tag>
            </div>
            <div style={{ marginTop: 4 }}>
              {data.namespace && (
                <Text type="secondary" style={{ fontSize: 12 }}>{data.namespace} / </Text>
              )}
              <Text strong style={{ fontSize: 14 }}>{data.name}</Text>
            </div>
          </div>
        )
      }
      placement="right"
      width={560}
      open={visible}
      onClose={onClose}
      styles={{ body: { padding: '12px 16px' } }}
    >
      {data && (
        <>
          {isGateway && (
            <GatewayListeners resource={data.resource} matchedListener={data._matchedListener} />
          )}

          <pre
            style={{
              margin: 0,
              padding: '12px 14px',
              background: 'var(--ec-color-bg-subtle)',
              border: '1px solid var(--ec-color-border)',
              borderRadius: 6,
              fontSize: 12,
              lineHeight: 1.6,
              overflowX: 'auto',
              whiteSpace: 'pre',
              color: 'var(--ec-color-text)',
              fontFamily: 'var(--ec-font-mono)',
            }}
          >
            {rawYaml}
          </pre>
        </>
      )}
    </Drawer>
  )
}
