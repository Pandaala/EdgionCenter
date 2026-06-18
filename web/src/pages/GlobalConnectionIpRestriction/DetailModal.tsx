import { useState, useEffect, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Modal, Button, Spin, Empty } from 'antd'
import * as yaml from 'js-yaml'
import YamlEditor from '@/components/YamlEditor'
import {
  globalConnectionIpRestrictionApi,
} from '@/api/globalConnectionIpRestriction'

interface Props {
  namespace: string
  name: string
  /** Raw controller id (may contain '/'). */
  controllerId: string
  open: boolean
  onClose: () => void
}

export default function DetailModal({ namespace, name, controllerId, open, onClose }: Props) {
  const [yamlContent, setYamlContent] = useState('')

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
    if (!open) return
    if (entry) {
      setYamlContent(yaml.dump(entry))
    }
  }, [entry, open])

  return (
    <Modal
      open={open}
      onCancel={onClose}
      width={900}
      destroyOnClose
      title={`${namespace}/${name} — ${controllerId}`}
      footer={<Button onClick={onClose}>Close</Button>}
    >
      {isLoading ? (
        <Spin size="large" style={{ display: 'flex', justifyContent: 'center', minHeight: 200 }} />
      ) : !entry ? (
        <Empty description={`Entry not found on controller "${controllerId}".`} />
      ) : (
        <YamlEditor
          value={yamlContent}
          onChange={setYamlContent}
          onValidate={() => {}}
          readOnly
          height="500px"
        />
      )}
    </Modal>
  )
}
