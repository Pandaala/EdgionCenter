/**
 * Listeners 列表编辑器
 */

import React from 'react'
import { Button } from 'antd'
import { PlusOutlined } from '@ant-design/icons'
import ListenerEditor from './ListenerEditor'
import type { GatewayListener } from '@/types/gateway-api/gateway'
import { useT } from '@/i18n'

interface ListenersSectionProps {
  value: GatewayListener[]
  onChange: (listeners: GatewayListener[]) => void
  disabled?: boolean
}

const defaultListener = (): GatewayListener => ({
  name: '',
  port: 80,
  protocol: 'HTTP',
})

const ListenersSection: React.FC<ListenersSectionProps> = ({
  value = [], onChange, disabled = false,
}) => {
  const t = useT()

  const updateListener = (index: number, listener: GatewayListener) => {
    const next = [...value]
    next[index] = listener
    onChange(next)
  }

  const addListener = () => onChange([...value, defaultListener()])
  const removeListener = (index: number) => onChange(value.filter((_, i) => i !== index))

  return (
    <div>
      {value.map((listener, index) => (
        <ListenerEditor
          key={index}
          listener={listener}
          index={index}
          canRemove={value.length > 1}
          onChange={(l) => updateListener(index, l)}
          onRemove={() => removeListener(index)}
          disabled={disabled}
        />
      ))}
      {!disabled && (
        <Button type="dashed" block icon={<PlusOutlined />} onClick={addListener}>
          {t('btn.addListener')}
        </Button>
      )}
    </div>
  )
}

export default ListenersSection
