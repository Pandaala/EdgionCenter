import { useState } from 'react'
import { Modal, Form, Input, Select, Space, message } from 'antd'
import { useMutation, useQuery } from '@tanstack/react-query'
import * as yaml from 'js-yaml'
import YamlEditor from '@/components/YamlEditor'
import {
  globalConnectionIpRestrictionApi,
  type GlobalConnectionIpRestrictionData,
} from '@/api/globalConnectionIpRestriction'
import { centerApi } from '@/api/center'

export interface CreateModalProps {
  open: boolean
  onClose: () => void
  onSuccess: () => void
}

const DEFAULT_YAML = `enable: true
activeProfile: default
description: "new PM"
profiles:
  default:
    defaultAction: deny
    allow:
      - name: office
        cidrs: ["192.168.1.0/24"]
`

export default function CreateModal({ open, onClose, onSuccess }: CreateModalProps) {
  const [form] = Form.useForm()
  const [yamlContent, setYamlContent] = useState(DEFAULT_YAML)
  const [yamlValid, setYamlValid] = useState(true)

  const { data: controllersResponse } = useQuery({
    queryKey: ['controllers'],
    queryFn: centerApi.listControllers,
    enabled: open,
  })

  // listControllers returns { success, data?: ControllerSummary[], count }
  // data is a flat array (not { items: [...] })
  const controllersList = controllersResponse?.data ?? []

  const controllerOptions = controllersList.map((c) => ({
    label: c.controller_id,
    value: c.controller_id,
  }))

  const createMutation = useMutation({
    mutationFn: async ({
      namespace,
      name,
      controller,
      data,
    }: {
      namespace: string
      name: string
      controller: string
      data: GlobalConnectionIpRestrictionData
    }) =>
      globalConnectionIpRestrictionApi.create({
        namespace,
        name,
        controllers: [controller],
        data,
      }),
    onSuccess: (res) => {
      const fanOut = res?.data
      if (fanOut?.failed?.length > 0) {
        message.error(`Create failed: ${fanOut.failed[0].error ?? 'unknown'}`)
      } else {
        message.success('Created')
        onSuccess()
        onClose()
        form.resetFields()
        setYamlContent(DEFAULT_YAML)
      }
    },
    onError: (e: Error) => message.error(`Create error: ${e.message}`),
  })

  const handleSubmit = () => {
    form.validateFields().then((values) => {
      try {
        const parsed = yaml.load(yamlContent) as GlobalConnectionIpRestrictionData
        if (!parsed || typeof parsed.enable !== 'boolean' || !parsed.activeProfile || !parsed.profiles) {
          message.error('YAML must include enable, activeProfile, profiles')
          return
        }
        createMutation.mutate({
          namespace: values.namespace,
          name: values.name,
          controller: values.controller,
          data: parsed,
        })
      } catch (e: unknown) {
        message.error(`YAML parse error: ${(e as Error).message}`)
      }
    })
  }

  return (
    <Modal
      open={open}
      title="New GlobalConnectionIpRestriction"
      onCancel={onClose}
      onOk={handleSubmit}
      okButtonProps={{ disabled: !yamlValid, loading: createMutation.isPending }}
      width={720}
    >
      <Form form={form} layout="vertical" initialValues={{ namespace: 'edgion-system' }}>
        <Space.Compact style={{ width: '100%' }}>
          <Form.Item name="namespace" label="Namespace" rules={[{ required: true }]} style={{ flex: 1 }}>
            <Input placeholder="edgion-system" />
          </Form.Item>
          <Form.Item name="name" label="Name" rules={[{ required: true }]} style={{ flex: 1, marginLeft: 12 }}>
            <Input placeholder="edge-restriction" />
          </Form.Item>
        </Space.Compact>
        <Form.Item name="controller" label="Target Controller (single)" rules={[{ required: true }]}>
          <Select options={controllerOptions} placeholder="Choose controller" />
        </Form.Item>
        <Form.Item label="Data (YAML)">
          <YamlEditor
            value={yamlContent}
            onChange={setYamlContent}
            onValidate={(ok: boolean) => setYamlValid(ok)}
            height="320px"
          />
        </Form.Item>
      </Form>
    </Modal>
  )
}
