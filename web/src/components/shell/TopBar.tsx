import { useNavigate, useParams } from 'react-router-dom'
import { Button, Space, Dropdown } from 'antd'
import {
  ReloadOutlined,
  GlobalOutlined,
  LogoutOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  SearchOutlined,
  ArrowLeftOutlined,
} from '@ant-design/icons'
import { useT, useLanguage } from '@/i18n'
import { ThemeToggle } from '@/components/widgets/ThemeToggle'
import { authApi } from '@/api/auth'
import { clearLoggedIn } from '@/utils/auth'
import { getAppMode } from '@/utils/proxy'

interface TopBarProps {
  collapsed: boolean
  onToggleCollapse: () => void
}

export const TopBar = ({ collapsed, onToggleCollapse }: TopBarProps) => {
  const t = useT()
  const navigate = useNavigate()
  const { lang, setLang } = useLanguage()
  const { controllerId: rawId } = useParams<{ controllerId?: string }>()
  const activeControllerId = rawId?.replace(/~/g, '/') ?? null
  const isCenterMode = getAppMode() === 'center'

  const handleLogout = async () => {
    await authApi.logout()
    clearLoggedIn()
    navigate('/login')
  }

  return (
    <header
      style={{
        height: 56,
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        padding: '0 20px',
        background: 'var(--ec-color-bg-surface)',
        borderBottom: '1px solid var(--ec-color-border)',
      }}
    >
      <Button
        data-testid="nav-toggle"
        type="text"
        icon={collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
        onClick={onToggleCollapse}
      />
      <div
        style={{
          fontSize: 16,
          fontWeight: 600,
          color: 'var(--ec-color-text)',
          letterSpacing: '-0.01em',
        }}
      >
        Edgion
      </div>
      {isCenterMode && activeControllerId && (
        <>
          <Button
            type="link"
            size="small"
            icon={<ArrowLeftOutlined />}
            onClick={() => navigate('/')}
            style={{ paddingLeft: 0 }}
          >
            {t('center.backToCenter')}
          </Button>
          <span
            style={{
              padding: '4px 10px',
              borderRadius: 'var(--ec-radius-sm)',
              background: 'var(--ec-color-bg-subtle)',
              color: 'var(--ec-color-text-muted)',
              fontSize: 'var(--ec-size-sm)',
            }}
          >
            {activeControllerId}
          </span>
        </>
      )}
      <div style={{ flex: 1 }} />
      <Space>
        <Button
          icon={<SearchOutlined />}
          disabled
          title="⌘K (coming soon)"
          style={{ color: 'var(--ec-color-text-subtle)' }}
        >
          ⌘K
        </Button>
        <ThemeToggle />
        <Button
          data-testid="language-toggle"
          type="text"
          icon={<GlobalOutlined />}
          onClick={() => setLang(lang === 'en' ? 'zh' : 'en')}
        >
          {lang === 'en' ? '中文' : 'EN'}
        </Button>
        <Button data-testid="page-reload" type="text" icon={<ReloadOutlined />} onClick={() => window.location.reload()} />
        <Dropdown
          menu={{
            items: [
              { key: 'logout', icon: <LogoutOutlined />, label: <span data-testid="logout">{t('login.logout')}</span>, onClick: handleLogout },
            ],
          }}
        >
          <Button data-testid="user-menu" type="text" icon={<LogoutOutlined />} />
        </Dropdown>
      </Space>
    </header>
  )
}
