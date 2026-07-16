const LOGGED_IN_KEY = 'edgion-logged-in'
const LOGIN_RETURN_PATH_KEY = 'edgion-login-return-path'

// Track login state via a simple, non-sensitive flag (not the actual token).
// localStorage intentionally mirrors the lifetime of the persistent HttpOnly
// session cookie; the API interceptor clears it whenever that cookie is no
// longer accepted. sessionStorage caused a valid session to be discarded on a
// new tab or browser context.
export function setLoggedIn(): void {
  localStorage.setItem(LOGGED_IN_KEY, '1')
}

export function clearLoggedIn(): void {
  localStorage.removeItem(LOGGED_IN_KEY)
}

// Quick sync check — may be stale, but avoids flash of login page
export function isLoggedIn(): boolean {
  return localStorage.getItem(LOGGED_IN_KEY) === '1'
}

function isSafeAppPath(path: string): boolean {
  return path.startsWith('/') && !path.startsWith('//')
}

export function saveLoginReturnPath(path: string): void {
  if (isSafeAppPath(path) && path !== '/login') {
    sessionStorage.setItem(LOGIN_RETURN_PATH_KEY, path)
  }
}

export function takeLoginReturnPath(): string | undefined {
  const path = sessionStorage.getItem(LOGIN_RETURN_PATH_KEY)
  sessionStorage.removeItem(LOGIN_RETURN_PATH_KEY)
  return path && isSafeAppPath(path) ? path : undefined
}
