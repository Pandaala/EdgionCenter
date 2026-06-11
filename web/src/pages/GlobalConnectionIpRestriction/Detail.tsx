import { useState, useEffect, useMemo } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Button, Spin, message, Card, Typography } from 'antd'
import { ArrowLeftOutlined, EditOutlined, SaveOutlined, CloseOutlined } from '@ant-design/icons'
import * as yaml from 'js-yaml'
import YamlEditor from '@/components/YamlEditor'
import PageHeader from '@/components/PageHeader'
import {
  globalConnectionIpRestrictionApi,
  type GlobalConnectionIpRestrictionData,
  type CenterGlobalIpRestrictionView,
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
  const queryClient = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [yamlContent, setYamlContent] = useState('')
  const [yamlValid, setYamlValid] = useState(true)

  const { data: response, isLoading } = useQuery({
    queryKey: ['global-connection-ip-restriction-detail', namespace, name],
    queryFn: () => globalConnectionIpRestrictionApi.get(namespace!, name!),
    enabled: !!namespace && !!name,
  })

  const view: CenterGlobalIpRestrictionView | undefined = response?.data

  const entry = useMemo(
    () => (view && controllerId ? view.controllers[controllerId] : undefined),
    [view, controllerId]
  )

  useEffect(() => {
    if (entry) {
      const dataOnly: GlobalConnectionIpRestrictionData = {
        enable: entry.enable,
        activeProfile: entry.activeProfile,
        profiles: entry.profiles,
        description: entry.description,
      }
      setYamlContent(yaml.dump(dataOnly))
    }
  }, [entry])

  const updateMutation = useMutation({
    mutationFn: (payload: GlobalConnectionIpRestrictionData) =>
      globalConnectionIpRestrictionApi.update(namespace!, name!, {
        controllers: [controllerId!],
        data: payload,
      }),
    onSuccess: (res) => {
      const fanOut = res?.data
      if (fanOut?.failed?.length > 0) {
        message.error(`Update failed: ${fanOut.failed[0].error ?? 'unknown'}`)
      } else {
        message.success('Updated')
        setEditing(false)
        queryClient.invalidateQueries({ queryKey: ['global-connection-ip-restriction-detail', namespace, name] })
        queryClient.invalidateQueries({ queryKey: ['global-connection-ip-restrictions'] })
      }
    },
    onError: (e: Error) => message.error(`Update error: ${e.message}`),
  })

  const handleSave = () => {
    try {
      const parsed = yaml.load(yamlContent) as GlobalConnectionIpRestrictionData
      if (!parsed || typeof parsed.enable !== 'boolean' || !parsed.activeProfile || !parsed.profiles) {
        message.error('Invalid structure: requires enable, activeProfile, profiles')
        return
      }
      updateMutation.mutate(parsed)
    } catch (e: unknown) {
      message.error(`YAML parse error: ${(e as Error).message}`)
    }
  }

  if (isLoading) return <Spin size="large" style={{ display: 'flex', justifyContent: 'center', minHeight: 300 }} />
  if (!entry) {
    return (
      <Card>
        <Text type="secondary">PM not found on controller "{controllerId}".</Text>
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
          <>
            <Button icon={<ArrowLeftOutlined />} onClick={() => navigate('/global-connection-ip-restrictions')}>
              Back
            </Button>
            {editing ? (
              <>
                <Button icon={<CloseOutlined />} onClick={() => setEditing(false)}>
                  Cancel
                </Button>
                <Button
                  type="primary"
                  icon={<SaveOutlined />}
                  disabled={!yamlValid}
                  loading={updateMutation.isPending}
                  onClick={handleSave}
                >
                  Save
                </Button>
              </>
            ) : (
              <Button icon={<EditOutlined />} onClick={() => setEditing(true)}>
                Edit
              </Button>
            )}
          </>
        }
      />
      <YamlEditor
        value={yamlContent}
        onChange={setYamlContent}
        onValidate={(ok: boolean) => setYamlValid(ok)}
        readOnly={!editing}
        height="600px"
      />
    </div>
  )
}
