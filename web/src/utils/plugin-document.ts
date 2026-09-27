/** Validate renderable plugin lists without normalizing or filtering operator data. */
export function validatePluginStages(spec: Record<string, unknown>, stages: readonly string[]): void {
  for (const stage of stages) {
    const entries = spec[stage]
    if (entries == null) continue
    if (!Array.isArray(entries)) throw new Error(`${stage} must be an array`)
    entries.forEach((entry, index) => {
      const path = `${stage}[${index}]`
      if (!entry || typeof entry !== 'object' || Array.isArray(entry) || typeof entry.type !== 'string') throw new Error(`${path} must be a typed plugin object`)
      if (entry.config != null && (typeof entry.config !== 'object' || Array.isArray(entry.config))) throw new Error(`${path}.config must be an object`)
    })
  }
}
