import Editor, { loader } from '@monaco-editor/react'
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api.js'
import 'monaco-editor/esm/vs/editor/editor.all.js'
import 'monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js'
import 'monaco-editor/esm/vs/language/json/monaco.contribution.js'
import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker.js?worker'
import JsonWorker from 'monaco-editor/esm/vs/language/json/json.worker.js?worker'

// Bundle the editor and its workers locally so editing never depends on a CDN.
globalThis.MonacoEnvironment = {
  getWorker: (_moduleId, label) => label === 'json' ? new JsonWorker() : new EditorWorker(),
}
loader.config({ monaco })

export default Editor
