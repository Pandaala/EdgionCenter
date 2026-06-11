let _activeControllerId: string | null = null
let _mode: 'controller' | 'center' | null = null

export function setActiveControllerId(id: string | null) {
  _activeControllerId = id
}

export function getActiveControllerId(): string | null {
  return _activeControllerId
}

export function setAppMode(mode: 'controller' | 'center') {
  _mode = mode
}

export function getAppMode(): 'controller' | 'center' | null {
  return _mode
}
