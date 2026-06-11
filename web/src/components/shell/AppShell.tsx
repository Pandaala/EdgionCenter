import { useState } from 'react'
import { Outlet } from 'react-router-dom'
import { Sidebar } from './Sidebar'
import { TopBar } from './TopBar'
import type { AppMode } from './menuConfig'

interface AppShellProps {
  mode?: AppMode
}

export const AppShell = ({ mode = 'controller' }: AppShellProps) => {
  const [collapsed, setCollapsed] = useState(false)
  return (
    <div style={{ display: 'flex', minHeight: '100vh', background: 'var(--ec-color-bg-base)' }}>
      <Sidebar collapsed={collapsed} mode={mode} />
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <TopBar collapsed={collapsed} onToggleCollapse={() => setCollapsed((c) => !c)} />
        <main
          style={{
            flex: 1,
            padding: 24,
            overflow: 'auto',
            background: 'var(--ec-color-bg-base)',
          }}
        >
          <Outlet />
        </main>
      </div>
    </div>
  )
}

export default AppShell
