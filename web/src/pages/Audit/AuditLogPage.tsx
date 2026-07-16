import { useState } from 'react'
import { useQuery, keepPreviousData } from '@tanstack/react-query'
import { Table, Input, Button, Space, Tag, Typography } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import dayjs from 'dayjs'
import { auditApi, type AuditRecordDto, type AuditListParams } from '@/api/audit'
import { useT } from '@/i18n'
import PageHeader from '@/components/PageHeader'

const { Text } = Typography

const PAGE_SIZE = 50

interface Filters {
  actor: string
  controller: string
  since: string
  until: string
}

const EMPTY_FILTERS: Filters = { actor: '', controller: '', since: '', until: '' }

/** Parse a datetime-local string ("YYYY-MM-DDTHH:mm") into unix seconds. */
function toUnix(value: string): number | undefined {
  if (!value) return undefined
  const d = dayjs(value)
  return d.isValid() ? d.unix() : undefined
}

function statusColor(status: number): string {
  if (status >= 500) return 'red'
  if (status >= 400) return 'orange'
  if (status >= 200 && status < 300) return 'green'
  return 'default'
}

export default function AuditLogPage() {
  const t = useT()
  // `draft` holds the live input values; `applied` is what actually drives the
  // query so typing doesn't refetch on every keystroke.
  const [draft, setDraft] = useState<Filters>(EMPTY_FILTERS)
  const [applied, setApplied] = useState<Filters>(EMPTY_FILTERS)
  const [page, setPage] = useState(0)

  const params: AuditListParams = {
    limit: PAGE_SIZE,
    offset: page * PAGE_SIZE,
    actor: applied.actor || undefined,
    controller: applied.controller || undefined,
    since: toUnix(applied.since),
    until: toUnix(applied.until),
  }

  const { data, isFetching, refetch } = useQuery({
    queryKey: ['center-audit-logs', params],
    queryFn: () => auditApi.list(params),
    placeholderData: keepPreviousData,
    staleTime: 10000,
  })

  const rows: AuditRecordDto[] = data?.data ?? []

  const applyFilters = () => {
    setPage(0)
    setApplied(draft)
  }

  const resetFilters = () => {
    setPage(0)
    setDraft(EMPTY_FILTERS)
    setApplied(EMPTY_FILTERS)
  }

  const columns = [
    {
      title: t('audit.col.ts'),
      dataIndex: 'ts',
      key: 'ts',
      width: 180,
      render: (v: number) => dayjs.unix(v).format('YYYY-MM-DD HH:mm:ss'),
    },
    {
      title: t('audit.col.actor'),
      dataIndex: 'actor',
      key: 'actor',
      render: (v: string) => <Text strong>{v}</Text>,
    },
    {
      title: t('audit.col.provider'),
      dataIndex: 'provider',
      key: 'provider',
      render: (v: string) => (v ? <Tag>{v}</Tag> : '-'),
    },
    {
      title: t('audit.col.method'),
      dataIndex: 'method',
      key: 'method',
      width: 90,
      render: (v: string) => <Tag color="blue">{v}</Tag>,
    },
    {
      title: t('audit.col.path'),
      dataIndex: 'path',
      key: 'path',
      render: (v: string) => <Text code>{v}</Text>,
    },
    {
      title: t('audit.col.targetController'),
      dataIndex: 'targetController',
      key: 'targetController',
      render: (v?: string | null) => (v ? <Tag color="purple">{v}</Tag> : '-'),
    },
    {
      title: t('audit.col.status'),
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (v: number) => <Tag color={statusColor(v)}>{v}</Tag>,
    },
    {
      title: t('audit.col.sourceIp'),
      dataIndex: 'sourceIp',
      key: 'sourceIp',
      render: (v?: string | null) => v || '-',
    },
  ]

  return (
    <div>
      <PageHeader
        title={t('audit.title')}
        subtitle={t('audit.subtitle')}
        actions={
          <Button data-testid="audit-refresh" icon={<ReloadOutlined />} onClick={() => refetch()}>
            {t('btn.refresh')}
          </Button>
        }
      />

      <Space wrap style={{ marginBottom: 16 }}>
        <Input
          data-testid="audit-actor-filter"
          placeholder={t('audit.filter.actor')}
          allowClear
          style={{ width: 180 }}
          value={draft.actor}
          onChange={(e) => setDraft({ ...draft, actor: e.target.value })}
          onPressEnter={applyFilters}
        />
        <Input
          data-testid="audit-controller-filter"
          placeholder={t('audit.filter.controller')}
          allowClear
          style={{ width: 200 }}
          value={draft.controller}
          onChange={(e) => setDraft({ ...draft, controller: e.target.value })}
          onPressEnter={applyFilters}
        />
        <Input
          data-testid="audit-since-filter"
          type="datetime-local"
          aria-label={t('audit.filter.since')}
          style={{ width: 220 }}
          value={draft.since}
          onChange={(e) => setDraft({ ...draft, since: e.target.value })}
        />
        <Input
          data-testid="audit-until-filter"
          type="datetime-local"
          aria-label={t('audit.filter.until')}
          style={{ width: 220 }}
          value={draft.until}
          onChange={(e) => setDraft({ ...draft, until: e.target.value })}
        />
        <Button data-testid="audit-apply" type="primary" onClick={applyFilters}>
          {t('audit.filter.apply')}
        </Button>
        <Button data-testid="audit-reset" onClick={resetFilters}>{t('audit.filter.reset')}</Button>
      </Space>

      <Table
        dataSource={rows}
        columns={columns}
        rowKey={(r) => `${r.ts}-${r.actor}-${r.path}-${r.requestId ?? ''}`}
        loading={isFetching}
        pagination={false}
        size="middle"
      />

      <Space style={{ marginTop: 16, justifyContent: 'flex-end', width: '100%' }}>
        <Button data-testid="audit-prev" disabled={page === 0 || isFetching} onClick={() => setPage((p) => Math.max(0, p - 1))}>
          {t('audit.page.prev')}
        </Button>
        <Text>{t('audit.page.current', { n: page + 1 })}</Text>
        <Button data-testid="audit-next" disabled={rows.length < PAGE_SIZE || isFetching} onClick={() => setPage((p) => p + 1)}>
          {t('audit.page.next')}
        </Button>
      </Space>
    </div>
  )
}
