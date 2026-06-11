import * as yaml from 'js-yaml'

/**
 * Recursively removes empty values (null, undefined, empty strings, empty objects, empty arrays)
 * from an object tree. Used to produce clean YAML output.
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
  return yaml.dump(removeEmpty(obj), { lineWidth: -1, noRefs: true })
}
