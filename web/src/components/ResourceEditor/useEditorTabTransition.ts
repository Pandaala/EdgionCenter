import { useCallback, useRef, useState } from 'react'

export type EditorTab = 'form' | 'yaml' | 'conditions'
type EditableTab = Exclude<EditorTab, 'conditions'>

interface Options<T> {
  formData: T
  yamlContent: string
  serialize: (value: T) => string
  parse: (source: string) => T
  setFormData: (value: T) => void
  setYamlContent: (value: string) => void
  onError: (error: Error) => void
}

/**
 * Keeps Conditions transparent while synchronizing only real Form/YAML
 * transitions. A failed YAML parse leaves the user on YAML with both drafts
 * intact instead of revealing stale form data.
 */
export function useEditorTabTransition<T>(options: Options<T>) {
  const [activeTab, setActiveTab] = useState<EditorTab>('form')
  const lastEditableTab = useRef<EditableTab>('form')

  const resetEditorTab = useCallback((tab: EditableTab = 'form') => {
    lastEditableTab.current = tab
    setActiveTab(tab)
  }, [])

  const handleTabChange = (target: string) => {
    if (target === 'conditions') {
      setActiveTab('conditions')
      return
    }
    if (target !== 'form' && target !== 'yaml') return
    const source = activeTab === 'conditions' ? lastEditableTab.current : activeTab
    try {
      if (source === 'form' && target === 'yaml') options.setYamlContent(options.serialize(options.formData))
      if (source === 'yaml' && target === 'form') options.setFormData(options.parse(options.yamlContent))
      lastEditableTab.current = target
      setActiveTab(target)
    } catch (error) {
      options.onError(error instanceof Error ? error : new Error(String(error)))
    }
  }

  const editableTab = activeTab === 'conditions' ? lastEditableTab.current : activeTab
  return { activeTab, editableTab, resetEditorTab, handleTabChange }
}
