import type { ControllerSlot } from './case-ledger.ts'

export function controllerName(slot: ControllerSlot, required = true): string {
  const value = process.env[`E2E_CONTROLLER_${slot}`]
  if (!value && required) throw new Error(`E2E_CONTROLLER_${slot} is required`)
  return value ?? `controller-${slot.toLowerCase()}`
}

export function controllerCluster(slot: ControllerSlot): string {
  return slot === 'A' ? 'e2e-a' : 'e2e-b'
}

export function controllerId(slot: ControllerSlot, required = true): string {
  return `${controllerCluster(slot)}/${controllerName(slot, required)}`
}

export function controllerPathId(slot: ControllerSlot, required = true): string {
  return controllerId(slot, required).replaceAll('/', '~')
}
