/**
 * 插件阶段概览
 * 展示 EdgionPlugins 的四个执行阶段及各阶段中的插件列表
 */

import React from 'react'
import { Card, Collapse, Tag, Space, Badge, Typography, Empty } from 'antd'
import type { EdgionPluginsSpec, PluginEntry } from '@/types/edgion-plugins'
import { useT } from '@/i18n'

const { Text } = Typography

interface PluginStagesSectionProps {
  value?: EdgionPluginsSpec
}

const PluginStagesSection: React.FC<PluginStagesSectionProps> = ({ value = {} }) => {
  const t = useT()

  interface StageConfig {
    key: keyof EdgionPluginsSpec
    labelKey: string
    description: string
  }

  const STAGES: StageConfig[] = [
    {
      key: 'requestPlugins',
      labelKey: 'plugins.requestStage',
      description: 'Async execution — handles inbound requests (auth, rate-limit, rewrite, etc.)',
    },
    {
      key: 'upstreamResponseFilterPlugins',
      labelKey: 'plugins.responseFilter',
      description: 'Sync execution — processes upstream response headers',
    },
    {
      key: 'upstreamResponseBodyFilterPlugins',
      labelKey: 'plugins.responseBody',
      description: 'Sync execution — processes response body (bandwidth limit, etc.)',
    },
    {
      key: 'upstreamResponsePlugins',
      labelKey: 'plugins.upstreamResponse',
      description: 'Async execution — runs after upstream response completes',
    },
  ]

  const renderPluginList = (plugins: PluginEntry[] | undefined) => {
    if (!plugins?.length) {
      return (
        <Empty
          description={t('plugins.noPlugins')}
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          style={{ margin: '8px 0' }}
        />
      )
    }
    return (
      <Space direction="vertical" style={{ width: '100%' }}>
        {plugins.map((plugin, index) => (
          <div
            key={index}
            style={{
              padding: '8px 12px',
              background: 'var(--ec-color-bg-subtle)',
              borderRadius: 4,
              border: '1px solid var(--ec-color-border)',
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
            }}
          >
            <Space>
              <Tag color="blue">{plugin.type}</Tag>
              {plugin.enable === false && <Tag color="orange">Disabled</Tag>}
              {plugin.conditions && <Tag color="purple">Conditional</Tag>}
            </Space>
          </div>
        ))}
      </Space>
    )
  }

  const collapseItems = STAGES.map(({ key, labelKey, description }) => {
    const plugins = value[key] as PluginEntry[] | undefined
    const count = plugins?.length ?? 0
    return {
      key,
      label: (
        <Space>
          <Text>{t(labelKey)}</Text>
          <Badge
            count={count}
            showZero
            style={{ backgroundColor: count > 0 ? '#1677ff' : '#d9d9d9' }}
          />
        </Space>
      ),
      children: (
        <>
          <Text type="secondary" style={{ display: 'block', marginBottom: 8, fontSize: 12 }}>
            {description}
          </Text>
          {renderPluginList(plugins)}
        </>
      ),
    }
  })

  return (
    <Card title="Plugin Configuration" size="small">
      <Text type="secondary" style={{ display: 'block', marginBottom: 12 }}>
        This is a read-only overview. To edit plugin details, switch to YAML mode.
      </Text>
      <Collapse items={collapseItems} />
    </Card>
  )
}

export default PluginStagesSection
