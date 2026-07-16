import type { ReactNode } from 'react'

export const editorCancelButtonProps = { 'data-testid': 'editor-cancel' } as const
export const editorSubmitButtonProps = { 'data-testid': 'editor-submit' } as const

export function editorFormTab(label: ReactNode): ReactNode {
  return <span data-testid="editor-form-tab">{label}</span>
}

export function editorYamlTab(label: ReactNode): ReactNode {
  return <span data-testid="editor-yaml-tab">{label}</span>
}

export function editorConditionsTab(label: ReactNode): ReactNode {
  return <span data-testid="editor-conditions-tab">{label}</span>
}
