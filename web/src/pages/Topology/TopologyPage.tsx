import { useState, useCallback } from 'react'
import { Select, Button, Spin, Alert, Empty } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { useT } from '@/i18n'
import TopologyCanvas from './components/TopologyCanvas'
import TopologyLegend from './components/TopologyLegend'
import TopologyDetailDrawer from './components/TopologyDetailDrawer'
import { useTopologyData } from './hooks/useTopologyData'
import PageHeader from '@/components/PageHeader'

export default function TopologyPage() {
  const t = useT()
  const [namespaceFilter, setNamespaceFilter] = useState<string | null>(null)
  const [selectedNode, setSelectedNode] = useState<any | null>(null)
  const [drawerVisible, setDrawerVisible] = useState(false)

  const { nodes, edges, namespaces, plugins, gateways, isLoading, isError, refetch } = useTopologyData(namespaceFilter)

  const handleNodeClick = useCallback((nodeData: Record<string, any>) => {
    setSelectedNode(nodeData)
    setDrawerVisible(true)
  }, [])

  return (
    <div style={{ height: 'calc(100vh - 140px)', display: 'flex', flexDirection: 'column' }}>
      <PageHeader
        title="Topology"
        subtitle={t('page.subtitle.topology')}
        actions={
          <>
            <Select
              allowClear
              placeholder={t('topology.allNamespaces')}
              style={{ width: 200 }}
              value={namespaceFilter}
              onChange={(val) => setNamespaceFilter(val ?? null)}
              options={namespaces.map((ns) => ({ label: ns, value: ns }))}
            />
            <TopologyLegend />
            <Button icon={<ReloadOutlined />} onClick={() => refetch()}>{t('btn.refresh')}</Button>
          </>
        }
      />

      {/* Canvas */}
      <div style={{ flex: 1, border: '1px solid var(--ec-color-border)', borderRadius: 8, overflow: 'auto', background: 'var(--ec-color-bg-subtle)' }}>
        {isLoading ? (
          <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100%' }}>
            <Spin size="large" tip={t('topology.loading')} />
          </div>
        ) : isError ? (
          <Alert type="error" message={t('topology.error')} showIcon style={{ margin: 24 }} />
        ) : nodes.length === 0 ? (
          <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100%' }}>
            <Empty description={t('topology.noData')} />
          </div>
        ) : (
          <TopologyCanvas nodes={nodes} edges={edges} plugins={plugins} gateways={gateways} onNodeClick={handleNodeClick} />
        )}
      </div>

      <TopologyDetailDrawer
        visible={drawerVisible}
        data={selectedNode}
        onClose={() => setDrawerVisible(false)}
      />
    </div>
  )
}
