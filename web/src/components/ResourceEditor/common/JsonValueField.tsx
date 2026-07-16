import { useEffect, useState } from 'react'
import { Alert, Input } from 'antd'

export default function JsonValueField({ value, onChange, readOnly, expect = 'object' }: { value: unknown; onChange: (value: any) => void; readOnly?: boolean; expect?: 'object'|'array'|'any' }) {
  const [text, setText] = useState(() => JSON.stringify(value ?? (expect === 'array' ? [] : {}), null, 2))
  const [error, setError] = useState<string>()
  useEffect(() => { setText(JSON.stringify(value ?? (expect === 'array' ? [] : {}), null, 2)); setError(undefined) }, [value, expect])
  const apply = () => { try { const parsed=JSON.parse(text); if(expect==='object'&&(typeof parsed!=='object'||parsed===null||Array.isArray(parsed)))throw new Error('Expected a JSON object'); if(expect==='array'&&!Array.isArray(parsed))throw new Error('Expected a JSON array'); setError(undefined); onChange(parsed) } catch(e) { setError((e as Error).message) } }
  return <><Input.TextArea aria-label="JSON value" value={text} disabled={readOnly} rows={5} style={{fontFamily:'monospace'}} onChange={(e)=>setText(e.target.value)} onBlur={apply}/>{error&&<Alert type="error" showIcon message={error} style={{marginTop:4}}/>}</>
}
