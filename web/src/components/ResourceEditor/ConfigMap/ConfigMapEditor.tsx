import { useControllerMutationTarget } from '@/hooks/useControllerMutationTarget'
import { useEffect, useState } from 'react'
import { Alert, Button, Card, Form, Input, Modal, Space, Switch, Tabs, message } from 'antd'
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import YamlEditor from '@/components/YamlEditor'
import { editorCancelButtonProps, editorFormTab, editorSubmitButtonProps, editorYamlTab } from '../editorTestIds'
import MetadataSection from '../common/MetadataSection'
import { configMapFromYaml, configMapToYaml, createConfigMapReplacement, createEmptyConfigMap, type ConfigMapResource } from '@/utils/configmap'

type Props = { visible: boolean; mode: 'create' | 'edit'; resource?: any; onClose: () => void }

function MapEditor({ value, onChange }: { value: Record<string, string>; onChange: (v: Record<string, string>) => void }) {
  return <Space direction="vertical" style={{ width: '100%' }}>
    {Object.entries(value).map(([key, item]) => <Space key={key} align="start">
      <Input value={key} style={{ width: 180 }} onChange={(e) => { const next = { ...value }; delete next[key]; if (e.target.value) next[e.target.value] = item; onChange(next) }} />
      <Input.TextArea value={item} rows={2} style={{ width: 430 }} onChange={(e) => onChange({ ...value, [key]: e.target.value })} />
      <Button danger type="text" icon={<MinusCircleOutlined />} onClick={() => { const next = { ...value }; delete next[key]; onChange(next) }} />
    </Space>)}
    <Button block type="dashed" icon={<PlusOutlined />} onClick={() => onChange({ ...value, [`key-${Object.keys(value).length + 1}`]: '' })}>Add entry</Button>
  </Space>
}

export default function ConfigMapEditor({ visible, mode, resource, onClose }: Props) {
  const mutationTarget = useControllerMutationTarget()
  const [tab, setTab] = useState('form')
  const [draft, setDraft] = useState<ConfigMapResource>(createEmptyConfigMap)
  const [yaml, setYaml] = useState('')
  const qc = useQueryClient()
  useEffect(() => { if (!visible) return; const next = mode === 'create' ? createEmptyConfigMap() : createConfigMapReplacement(resource.metadata); setDraft(next); setYaml(configMapToYaml(next, mode === 'create' ? 'create' : 'update')); setTab('form') }, [visible, mode, resource])
  const mutation = useMutation({ mutationFn: async (value: ConfigMapResource) => {
    const ns = value.metadata.namespace; const body = configMapToYaml(value, mode === 'create' ? 'create' : 'update')
    if (!value.metadata.name || !ns) throw new Error('Name and namespace are required')
    if (mode === 'edit' && (value.metadata.name !== resource.metadata.name || ns !== resource.metadata.namespace)) throw new Error('ConfigMap cannot be renamed')
    return mode === 'create' ? resourceApi.create(mutationTarget, 'configmap', ns, body) : resourceApi.update(mutationTarget, 'configmap', ns, value.metadata.name, body)
  }, onSuccess: () => { message.success(mode === 'create' ? 'ConfigMap created' : 'ConfigMap replaced'); qc.invalidateQueries({ queryKey: ['restricted-keys', 'configmap'] }); onClose() }, onError: (e: Error) => message.error(e.message) })
  const switchTab = (key: string) => { try { if (key === 'yaml') setYaml(configMapToYaml(draft, mode === 'create' ? 'create' : 'update')); else setDraft(configMapFromYaml(yaml)); setTab(key) } catch (e) { message.error((e as Error).message) } }
  return <Modal open={visible} onCancel={onClose} width={850} title={`${mode === 'create' ? 'Create' : 'Replace'} ConfigMap`} footer={[<Button {...editorCancelButtonProps} key="cancel" onClick={onClose}>Cancel</Button>, <Button {...editorSubmitButtonProps} key="save" type="primary" loading={mutation.isPending} onClick={() => { try { mutation.mutate(tab === 'form' ? draft : configMapFromYaml(yaml)) } catch (e) { message.error((e as Error).message) } }}>{mode === 'create' ? 'Create' : 'Replace'}</Button>] }>
    {mode === 'edit' && <Alert type="warning" showIcon message="Full replacement" description="Existing values are not loaded. Enter the complete desired ConfigMap content." style={{ marginBottom: 12 }} />}
    <Tabs activeKey={tab} onChange={switchTab} items={[{ key: 'form', label: editorFormTab('Form'), children: <Form layout="vertical"><MetadataSection value={draft.metadata} isCreate={mode === 'create'} onChange={(metadata) => setDraft({ ...draft, metadata: { ...draft.metadata, ...metadata, namespace: metadata.namespace || '' } })} /><Card size="small" title="Text data"><MapEditor value={draft.data ?? {}} onChange={(data) => setDraft({ ...draft, data })} /></Card><Card size="small" title="Base64 binaryData" style={{ marginTop: 12 }}><MapEditor value={draft.binaryData ?? {}} onChange={(binaryData) => setDraft({ ...draft, binaryData })} /></Card><Form.Item label="Immutable" style={{ marginTop: 12 }}><Switch checked={draft.immutable} onChange={(immutable) => setDraft({ ...draft, immutable })} /></Form.Item></Form> }, { key: 'yaml', label: editorYamlTab('YAML'), children: <YamlEditor value={yaml} onChange={setYaml} height="500px" /> }]} />
  </Modal>
}
