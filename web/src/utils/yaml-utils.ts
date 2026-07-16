import * as yaml from 'js-yaml'

/**
 * Legacy create-template cleanup only. Never use for an existing resource or
 * an update payload: empty strings, arrays, and objects may be meaningful
 * operator state. Kept temporarily for callers that have not migrated yet.
 */
export function removeEmpty(obj: any): any {
  if (Array.isArray(obj)) {
    const arr = obj.map(removeEmpty).filter((v) => v !== null && v !== undefined)
    return arr.length > 0 ? arr : undefined
  }
  if (obj !== null && typeof obj === 'object') {
    const result: any = {}
    for (const [k, v] of Object.entries(obj)) {
      const cleaned = removeEmpty(v)
      if (cleaned !== null && cleaned !== undefined && cleaned !== '') result[k] = cleaned
    }
    return Object.keys(result).length > 0 ? result : undefined
  }
  return obj
}

export function dumpYaml(obj: any): string {
  return yaml.dump(obj, { lineWidth: -1, noRefs: true })
}
