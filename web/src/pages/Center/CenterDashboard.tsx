import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  Card,
  Row,
  Col,
  Tag,
  Badge,
  Button,
  Space,
  Input,
  Select,
  Statistic,
  Modal,
  Spin,
  Empty,
  message,
} from 'antd'
import {
  ReloadOutlined,
  ArrowRightOutlined,
  SyncOutlined,
} from '@ant-design/icons'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { centerApi, type ControllerSummary } from '@/api/center'
import { useT } from '@/i18n'

function formatLastSync(t: (key: string, params?: Record<string, string | number>) => string, secsAgo: number | null | undefined): string {
  if (secsAgo === null || secsAgo === undefined) return '—'
  if (secsAgo < 60) return t('center.secsAgo', { n: secsAgo })
  if (secsAgo < 3600) return t('center.minsAgo', { n: Math.floor(secsAgo / 60) })
  return t('center.hoursAgo', { n: Math.floor(secsAgo / 3600) })
}

const ControllerCard = ({
  controller,
  onReload,
}: {
  controller: ControllerSummary
  onReload: (id: string) => void
}) => {
  const t = useT()
  const navigate = useNavigate()

  const handleEnter = () => {
    navigate(`/controller/${controller.controller_id.replace(/\//g, '~')}`)
  }

  const handleReload = () => {
    Modal.confirm({
      title: t('center.reload'),
      content: t('center.reloadConfirm', { name: controller.controller_id }),
      onOk: () => onReload(controller.controller_id),
    })
  }

  return (
    <Card
      title={
        <Space>
          <Badge
            status={controller.online ? 'success' : 'error'}
            text={controller.online ? t('center.online') : t('center.offline')}
          />
          <span style={{ fontSize: 13, fontWeight: 600 }}>{controller.controller_id}</span>
        </Space>
      }
      size="small"
      extra={
        <Space size="small">
          <Button
            size="small"
            icon={<SyncOutlined />}
            onClick={handleReload}
          >
            {t('center.reload')}
          </Button>
          <Button
            size="small"
            type="primary"
            icon={<ArrowRightOutlined />}
            onClick={handleEnter}
          >
            {t('center.enter')}
          </Button>
        </Space>
      }
    >
      <Row gutter={[8, 8]}>
        <Col span={24}>
          <span style={{ color: 'var(--ec-color-text-muted)', fontSize: 12 }}>{t('center.cluster')}: </span>
          <Tag color="blue">{controller.cluster}</Tag>
        </Col>
        {controller.env.length > 0 && (
          <Col span={24}>
            <span style={{ color: 'var(--ec-color-text-muted)', fontSize: 12 }}>Env: </span>
            {controller.env.map((e) => (
              <Tag key={e} color="green">{e}</Tag>
            ))}
          </Col>
        )}
        {controller.tag.length > 0 && (
          <Col span={24}>
            <span style={{ color: 'var(--ec-color-text-muted)', fontSize: 12 }}>Tags: </span>
            {controller.tag.map((tg) => (
              <Tag key={tg}>{tg}</Tag>
            ))}
          </Col>
        )}
        <Col span={12}>
          <Statistic
            title={t('center.resourceCount')}
            value={controller.key_count ?? '—'}
            valueStyle={{ fontSize: 18, color: 'var(--ec-color-brand)' }}
          />
        </Col>
        <Col span={12}>
          <div style={{ paddingTop: 4 }}>
            <div style={{ color: 'var(--ec-color-text-muted)', fontSize: 12, marginBottom: 4 }}>{t('center.lastSync')}</div>
            <span style={{ fontSize: 13 }}>
              {formatLastSync(t, controller.last_seen_secs_ago ?? controller.last_list_secs_ago)}
            </span>
          </div>
        </Col>
      </Row>
    </Card>
  )
}

export default function CenterDashboard() {
  const t = useT()
  const queryClient = useQueryClient()
  const [searchText, setSearchText] = useState('')
  const [selectedCluster, setSelectedCluster] = useState<string | undefined>(undefined)

  const { data, isLoading } = useQuery({
    queryKey: ['center-controllers'],
    queryFn: centerApi.listControllers,
    staleTime: 30 * 1000,
  })

  const controllers = data?.data ?? []
  const clusters = [...new Set(controllers.map((c) => c.cluster))].sort()

  const filteredControllers = controllers.filter((c) => {
    const matchSearch =
      !searchText || c.controller_id.toLowerCase().includes(searchText.toLowerCase())
    const matchCluster = !selectedCluster || c.cluster === selectedCluster
    return matchSearch && matchCluster
  })

  const handleReload = async (id: string) => {
    try {
      await centerApi.reloadController(id)
      message.success(t('center.reloadOk'))
    } catch {
      // error handled by apiClient interceptor
    }
  }

  const handleRefresh = () => {
    queryClient.invalidateQueries({ queryKey: ['center-controllers'] })
  }

  return (
    <div>
      {/* Title omitted — left sidebar already identifies this view as Center. */}
      <Space style={{ marginBottom: 16 }} wrap>
        <Input.Search
          placeholder={t('center.searchPlaceholder')}
          allowClear
          style={{ width: 260 }}
          onSearch={(val) => setSearchText(val)}
          onChange={(e) => {
            if (!e.target.value) setSearchText('')
          }}
        />
        <Select
          style={{ width: 200 }}
          placeholder={t('center.filterCluster')}
          allowClear
          value={selectedCluster}
          onChange={(val) => setSelectedCluster(val)}
          options={[
            { value: undefined, label: t('center.allClusters') },
            ...clusters.map((c) => ({ value: c, label: c })),
          ]}
        />
        <Button icon={<ReloadOutlined />} onClick={handleRefresh}>
          {t('btn.refresh')}
        </Button>
      </Space>

      {isLoading ? (
        <Spin
          size="large"
          style={{
            display: 'flex',
            justifyContent: 'center',
            alignItems: 'center',
            minHeight: 200,
          }}
        />
      ) : filteredControllers.length === 0 ? (
        <Empty />
      ) : (
        <Row gutter={[16, 16]}>
          {filteredControllers.map((c) => (
            <Col key={c.controller_id} xs={24} sm={12} lg={8}>
              <ControllerCard controller={c} onReload={handleReload} />
            </Col>
          ))}
        </Row>
      )}
    </div>
  )
}
