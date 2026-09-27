import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Alert, Button, Drawer, Empty, Space, Spin, Tag, Typography } from 'antd'
import * as yaml from 'js-yaml'
import type {
  GlobalResourceApiSlug,
  GlobalResourceComparisonGroup,
  GlobalResourceComparisonMember,
} from '@/api/globalResources'
import { globalResourcesApi } from '@/api/globalResources'
import YamlEditor from '@/components/YamlEditor'
import { useT } from '@/i18n'
import dayjs from '@/lib/dayjs'
import { isGlobalConfigPayloadHidden } from './globalResourceConsistency'
import { SYNC_STATE_TAG_COLOR } from './globalResourceSyncState'

interface Props {
  apiSlug: GlobalResourceApiSlug
  group: GlobalResourceComparisonGroup | null
  onClose: () => void
}

function memberKey(member: GlobalResourceComparisonMember): string {
  return `${member.cluster}\u0000${member.controllerId ?? ''}`
}

export default function GlobalResourceComparisonDrawer({ apiSlug, group, onClose }: Props) {
  const t = useT()
  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const selectedMember = useMemo(
    () => group?.members.find((member) => memberKey(member) === selectedKey) ?? null,
    [group, selectedKey],
  )
  const detail = useQuery({
    queryKey: [
      'global-resources',
      'detail',
      apiSlug,
      group?.key.namespace,
      group?.key.name,
      selectedMember?.cluster,
    ],
    queryFn: () =>
      globalResourcesApi.detail(
        apiSlug,
        group!.key.namespace,
        group!.key.name,
        selectedMember!.cluster,
      ),
    enabled: group !== null && selectedMember !== null,
    retry: false,
  })
  const freshReadRejected = detail.isSuccess && (
    !detail.data.complete
    || detail.data.errors.length > 0
    || detail.data.object === null
  )
  const freshObject = detail.isSuccess && !freshReadRejected ? detail.data.object : null
  const object = freshObject ?? selectedMember?.object
  const showSnapshotWarning = detail.isError || freshReadRejected
  const yamlValue = useMemo(
    () => (object ? yaml.dump(object, { noRefs: true, lineWidth: -1 }) : ''),
    [object],
  )

  const close = () => {
    setSelectedKey(null)
    onClose()
  }

  return (
    <Drawer
      open={group !== null}
      onClose={close}
      destroyOnHidden
      width={920}
      title={
        group
          ? `${t('globalResources.drawer.title')} · ${group.key.namespace}/${group.key.name}`
          : t('globalResources.drawer.title')
      }
    >
      {group ? (
        <>
          <Typography.Paragraph type="secondary">
            {t('globalResources.drawer.selectMember')}
          </Typography.Paragraph>
          <Space wrap style={{ marginBottom: 16 }}>
            {group.members.map((member) => (
              <Button
                key={memberKey(member)}
                type={selectedKey === memberKey(member) ? 'primary' : 'default'}
                onClick={() => setSelectedKey(memberKey(member))}
                data-testid={`global-resource-member-${member.cluster}`}
              >
                {member.cluster}
                {' · '}
                {member.controllerId ?? t('globalResources.member.noController')}
              </Button>
            ))}
          </Space>

          {selectedMember?.freshnessUnixMs !== undefined ? (
            <Typography.Paragraph type="secondary" data-testid="global-resource-member-freshness">
              {t('globalResources.member.freshness')}
              {': '}
              {dayjs(selectedMember.freshnessUnixMs).fromNow()}
              {' '}
              <Tag color={SYNC_STATE_TAG_COLOR[selectedMember.syncState ?? 'ok']}>
                {t(`globalResources.syncState.${selectedMember.syncState ?? 'ok'}`)}
              </Tag>
            </Typography.Paragraph>
          ) : null}

          {selectedMember === null ? (
            <Empty description={t('globalResources.drawer.noMemberSelected')} />
          ) : (
            <>
              <Space wrap style={{ marginBottom: 12 }}>
                <Tag>{selectedMember.cluster}</Tag>
                <Tag>{selectedMember.controllerId ?? t('globalResources.member.noController')}</Tag>
                {detail.isFetching ? <Spin size="small" /> : null}
              </Space>
              {showSnapshotWarning ? (
                <Alert
                  type="warning"
                  showIcon
                  style={{ marginBottom: 12 }}
                  message={t('globalResources.drawer.detailFailed')}
                  description={
                    <>
                      <div>{t('globalResources.drawer.showingSnapshot')}</div>
                      {detail.isSuccess && detail.data.errors.length > 0 ? (
                        <ul data-testid="global-resource-detail-errors">
                          {detail.data.errors.map((error, index) => (
                            <li key={`${error.namespace}\u0000${error.code}\u0000${index}`}>
                              {error.namespace || t('globalResources.error.clusterScope')}
                              {' · '}
                              {t(`globalResources.errorCode.${error.code}`)}
                            </li>
                          ))}
                        </ul>
                      ) : null}
                      {detail.isSuccess && detail.data.errors.length === 0 ? (
                        <div>
                          {t(
                            detail.data.object === null
                              ? 'globalResources.drawer.freshObjectMissing'
                              : 'globalResources.drawer.freshReadIncomplete',
                          )}
                        </div>
                      ) : null}
                    </>
                  }
                />
              ) : null}
              {object && isGlobalConfigPayloadHidden(object) ? (
                <Alert type="info" showIcon message={t('globalResources.drawer.payloadHidden')} />
              ) : null}
              {object ? (
                <YamlEditor value={yamlValue} readOnly height="560px" />
              ) : (
                <Empty description={t('globalResources.drawer.objectUnavailable')} />
              )}
            </>
          )}
        </>
      ) : null}
    </Drawer>
  )
}
