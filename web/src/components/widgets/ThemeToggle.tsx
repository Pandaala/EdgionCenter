import { Segmented } from 'antd'
import { SunOutlined, MoonOutlined, DesktopOutlined } from '@ant-design/icons'
import { useTheme } from '@/theme'
import { useT } from '@/i18n'

export const ThemeToggle = () => {
  const { mode, setMode } = useTheme()
  const t = useT()
  return (
    <Segmented
      size="small"
      value={mode}
      onChange={(v) => setMode(v as 'light' | 'dark' | 'system')}
      options={[
        { value: 'light',  icon: <SunOutlined />,     label: t('theme.light') },
        { value: 'system', icon: <DesktopOutlined />, label: t('theme.system') },
        { value: 'dark',   icon: <MoonOutlined />,    label: t('theme.dark') },
      ]}
    />
  )
}
