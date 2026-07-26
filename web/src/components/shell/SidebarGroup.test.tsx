import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { SidebarGroup } from './SidebarGroup'

describe('SidebarGroup', () => {
  it('preserves an accessible label and title while collapsed', () => {
    render(
      <SidebarGroup label="EdgionConfigData" collapsed active depth={1}>
        <span>child</span>
      </SidebarGroup>,
    )

    const group = screen.getByLabelText('EdgionConfigData')
    expect(group).toHaveAttribute('title', 'EdgionConfigData')
    expect(screen.queryByText('EdgionConfigData')).not.toBeInTheDocument()
    expect(screen.getByText('child')).toBeInTheDocument()
  })

  it('renders the branch label while expanded', () => {
    render(
      <SidebarGroup label="GlobalResources" collapsed={false} active={false} depth={0}>
        <span>child</span>
      </SidebarGroup>,
    )

    expect(screen.getByText('GlobalResources')).toBeInTheDocument()
    expect(screen.queryByLabelText('GlobalResources')).not.toBeInTheDocument()
  })
})
