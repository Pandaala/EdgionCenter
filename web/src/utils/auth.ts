const LOGGED_IN_KEY = 'edgion-logged-in'

// Track login state via a simple flag (not the actual token)
export function setLoggedIn(): void {
  sessionStorage.setItem(LOGGED_IN_KEY, '1')
}

export function clearLoggedIn(): void {
  sessionStorage.removeItem(LOGGED_IN_KEY)
}

// Quick sync check — may be stale, but avoids flash of login page
export function isLoggedIn(): boolean {
  return sessionStorage.getItem(LOGGED_IN_KEY) === '1'
}
