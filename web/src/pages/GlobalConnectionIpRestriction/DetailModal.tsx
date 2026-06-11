import { useState, useEffect, useMemo } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Modal, Button, Spin, message, Empty, Space } from 'antd'
import { EditOutlined, SaveOutlined, CloseOutlined } from '@ant-design/icons'
import * as yaml from 'js-yaml'
import YamlEditor from '@/components/YamlEditor'
import {
  globalConnectionIpRestrictionApi,
  type GlobalConnectionIpRestrictionData,
} from '@/api/globalConnectionIpRestriction'

interface Props {
  namespace: string
  name: string
  /** Raw controller id (contains '/'). */
  controllerId: string
  open: boolean
  onClose: () => void
}

export default function DetailModal({ namespace, name, controllerId, open, onClose }: Props) {
  const queryClient = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [yamlContent, setYamlContent] = useState('')
  const [yamlValid, setYamlValid] = useState(true)

  const { data: response, isLoading } = useQuery({
    queryKey: ['global-connection-ip-restriction-detail', namespace, name],
    queryFn: () => globalConnectionIpRestrictionApi.get(namespace, name),
    enabled: open && !!namespace && !!name,
  })

  const entry = useMemo(
    () => response?.data?.controllers[controllerId],
    [response, controllerId],
  )

  useEffect(() => {
    if (!open) {
      // Reset editing state when modal closes so the next open starts fresh.
      setEditing(false)
      return
    }
    if (entry) {
      const dataOnly: GlobalConnectionIpRestrictionData = {
        enable: entry.enable,
        activeProfile: entry.activeProfile,
        profiles: entry.profiles,
        description: entry.description,
      }
      setYamlContent(yaml.dump(dataOnly))
    }
  }, [entry, open])

  const updateMutation = useMutation({
    mutationFn: (payload: GlobalConnectionIpRestrictionData) =>
      globalConnectionIpRestrictionApi.update(namespace, name, {
        controllers: [controllerId],
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

  return (
    <Modal
      open={open}
      onCancel={onClose}
      width={900}
      destroyOnClose
      title={`${namespace}/${name} — ${controllerId}`}
      footer={
        <Space>
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
            <>
              <Button onClick={onClose}>Close</Button>
              <Button type="primary" icon={<EditOutlined />} onClick={() => setEditing(true)} disabled={!entry}>
                Edit
              </Button>
            </>
          )}
        </Space>
      }
    >
      {isLoading ? (
        <Spin size="large" style={{ display: 'flex', justifyContent: 'center', minHeight: 200 }} />
      ) : !entry ? (
        <Empty description={`PM not found on controller "${controllerId}".`} />
      ) : (
        <YamlEditor
          value={yamlContent}
          onChange={setYamlContent}
          onValidate={(ok: boolean) => setYamlValid(ok)}
          readOnly={!editing}
          height="500px"
        />
      )}
    </Modal>
  )
}
