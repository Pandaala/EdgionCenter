import { useState, useEffect, useMemo } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { Button, Spin, Card, Typography } from 'antd'
import { ArrowLeftOutlined } from '@ant-design/icons'
import * as yaml from 'js-yaml'
import YamlEditor from '@/components/YamlEditor'
import PageHeader from '@/components/PageHeader'
import {
  globalConnectionIpRestrictionApi,
  type CenterGirAggregatedView,
} from '@/api/globalConnectionIpRestriction'

const { Text } = Typography

export default function GlobalConnectionIpRestrictionDetail() {
  const { namespace, name, controllerId: rawControllerId } = useParams<{
    namespace: string
    name: string
    controllerId: string
  }>()
  // List.tsx encoded '/' as '~' to fit a single URL segment; decode back so the
  // raw form ("cluster-east/ctrl-01") matches the controllers-map keys.
  const controllerId = rawControllerId?.replace(/~/g, '/')
  const navigate = useNavigate()
  const [yamlContent, setYamlContent] = useState('')

  const { data: response, isLoading } = useQuery({
    queryKey: ['global-connection-ip-restriction-detail', namespace, name],
    queryFn: () => globalConnectionIpRestrictionApi.get(namespace!, name!),
    enabled: !!namespace && !!name,
  })

  const view: CenterGirAggregatedView | undefined = response?.data

  const entry = useMemo(
    () => (view && controllerId ? view.controllers[controllerId] : undefined),
    [view, controllerId]
  )

  useEffect(() => {
    if (entry) {
      setYamlContent(yaml.dump(entry))
    }
  }, [entry])

  if (isLoading) return <Spin size="large" style={{ display: 'flex', justifyContent: 'center', minHeight: 300 }} />
  if (!entry) {
    return (
      <Card>
        <Text type="secondary">Entry not found on controller "{controllerId}".</Text>
        <Button style={{ marginTop: 16 }} onClick={() => navigate('/global-connection-ip-restrictions')}>
          Back to list
        </Button>
      </Card>
    )
  }

  return (
    <div>
      <PageHeader
        title={`${namespace}/${name}`}
        subtitle={`on ${controllerId}`}
        actions={
          <Button icon={<ArrowLeftOutlined />} onClick={() => navigate('/global-connection-ip-restrictions')}>
            Back
          </Button>
        }
      />
      <YamlEditor
        value={yamlContent}
        onChange={setYamlContent}
        onValidate={() => {}}
        readOnly
        height="600px"
      />
    </div>
  )
}
