import type { ThemeTokens } from './tokens'

const kebab = (s: string) =>
  s.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase())

/**
 * Convert a ThemeTokens object into a flat map of CSS custom property
 * declarations. Keys are written as `--ec-<group>-<name>`.
 */
export const tokensToCssVars = (t: ThemeTokens): Record<string, string> => {
  const out: Record<string, string> = {}
  out['--ec-font-sans'] = t.font.sans
  out['--ec-font-mono'] = t.font.mono
  for (const [k, v] of Object.entries(t.color)) out[`--ec-color-${kebab(k)}`] = v
  for (const [k, v] of Object.entries(t.shadow)) out[`--ec-shadow-${k}`] = v
  for (const [k, v] of Object.entries(t.radius)) out[`--ec-radius-${k}`] = `${v}px`
  for (const [k, v] of Object.entries(t.spacing)) out[`--ec-space-${k}`] = `${v}px`
  for (const [k, v] of Object.entries(t.size)) out[`--ec-size-${k}`] = `${v}px`
  return out
}

/**
 * Apply the CSS-variable map to `document.documentElement`.
 */
export const applyCssVars = (vars: Record<string, string>) => {
  const root = document.documentElement
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v)
}
