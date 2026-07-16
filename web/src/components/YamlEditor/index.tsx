import { useEffect, useRef, useState } from 'react'
import Editor, { OnMount } from '@monaco-editor/react'
import { Alert } from 'antd'
import * as yaml from 'js-yaml'
import type { editor } from 'monaco-editor'
import { useTheme } from '@/theme'

export interface YamlEditorProps {
  value?: string
  defaultValue?: string
  onChange?: (value: string) => void
  onValidate?: (isValid: boolean, error?: string) => void
  readOnly?: boolean
  height?: string | number
  language?: 'yaml' | 'json'
}

const YamlEditor = ({
  value,
  defaultValue = '',
  onChange,
  onValidate,
  readOnly = false,
  height = '500px',
  language = 'yaml',
}: YamlEditorProps) => {
  const { resolvedMode } = useTheme()
  const containerRef = useRef<HTMLDivElement | null>(null)
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)
  const [error, setError] = useState<string>('')

  const handleEditorDidMount: OnMount = (editor) => {
    editorRef.current = editor
  }

  const validateYaml = (content: string): { isValid: boolean; error?: string } => {
    if (!content.trim()) {
      return { isValid: true }
    }

    try {
      if (language === 'yaml') {
        yaml.load(content)
      } else {
        JSON.parse(content)
      }
      return { isValid: true }
    } catch (e: any) {
      const errorMsg = e.message || 'Parse error'
      return { isValid: false, error: errorMsg }
    }
  }

  const handleEditorChange = (value: string | undefined) => {
    const content = value || ''
    
    // Validate YAML/JSON.
    const validation = validateYaml(content)
    setError(validation.error || '')
    
    // Report validation state.
    if (onValidate) {
      onValidate(validation.isValid, validation.error)
    }
    
    // Report content changes.
    if (onChange) {
      onChange(content)
    }
  }

  // Revalidate when the controlled value changes.
  useEffect(() => {
    if (value !== undefined) {
      const validation = validateYaml(value)
      setError(validation.error || '')
      if (onValidate) {
        onValidate(validation.isValid, validation.error)
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value])

  useEffect(() => {
    const container = containerRef.current
    if (!container) return
    const replace = (event: Event) => {
      const content = (event as CustomEvent<unknown>).detail
      if (typeof content === 'string') editorRef.current?.setValue(content)
    }
    container.addEventListener('edgion:replace-yaml', replace)
    return () => container.removeEventListener('edgion:replace-yaml', replace)
  }, [])

  return (
    <div ref={containerRef} data-testid="yaml-editor" data-yaml-value={value} style={{ border: '1px solid var(--ec-color-border)', borderRadius: 'var(--ec-radius-sm)', overflow: 'hidden' }}>
      {error && (
        <Alert
          message="Syntax Error"
          description={error}
          type="error"
          showIcon
          style={{ margin: '8px', borderRadius: 'var(--ec-radius-sm)' }}
        />
      )}
      <Editor
        height={height}
        language={language}
        theme={resolvedMode === 'dark' ? 'vs-dark' : 'light'}
        value={value}
        defaultValue={defaultValue}
        onChange={handleEditorChange}
        onMount={handleEditorDidMount}
        options={{
          readOnly,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          fontSize: 14,
          lineNumbers: 'on',
          roundedSelection: false,
          automaticLayout: true,
          wordWrap: 'on',
          wrappingIndent: 'same',
          folding: true,
          renderLineHighlight: 'all',
          suggestOnTriggerCharacters: true,
          acceptSuggestionOnEnter: 'on',
          tabSize: 2,
        }}
      />
    </div>
  )
}

export default YamlEditor
