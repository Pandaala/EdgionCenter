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
    
    // 验证 YAML/JSON
    const validation = validateYaml(content)
    setError(validation.error || '')
    
    // 回调验证结果
    if (onValidate) {
      onValidate(validation.isValid, validation.error)
    }
    
    // 回调内容变化
    if (onChange) {
      onChange(content)
    }
  }

  // 当外部 value 改变时，验证它
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

  return (
    <div style={{ border: '1px solid var(--ec-color-border)', borderRadius: 'var(--ec-radius-sm)', overflow: 'hidden' }}>
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

