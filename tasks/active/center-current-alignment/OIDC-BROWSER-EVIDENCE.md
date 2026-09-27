# Native OIDC browser evidence

Recorded on 2026-09-28 against Center commit `81d83a2` (no production changes).
This is a standalone OIDC composition, not a Kubernetes deployment result.

## Runtime

- Native `target/debug/edgion-center-standalone`, SQLite, OIDC only, `allow_all`;
  no local password provider. Admin/probe/federation retain their normal ports.
- Current dashboard served by Vite on 15173; its existing API proxy reaches
  native Center. No Controller or managed Kubernetes API participates.
- Dex `ghcr.io/dexidp/dex:v2.41.1`, HTTPS issuer
  `https://127.0.0.1:15556/dex`, client `edgion-e2e`, PKCE S256.
- oauth2-proxy `quay.io/oauth2-proxy/oauth2-proxy:v7.7.1`, browser entry
  `http://127.0.0.1:14180`, callback `/oauth2/callback`, forwards the ID token
  as Authorization. Its upstream is the native Vite server.
- Dedicated local CA: Center and oauth2-proxy validate discovery/JWKS TLS.
  Chromium trusts only the test leaf's SPKI via its launch argument. No global
  certificate validation bypass, host trust-store edit or DNS edit was used.
- Federation remains configured with its existing test mTLS certificates.
  The new Center database is separate from the full native regression database.
- Private artifact directory: `/tmp/ws5-center-oidc-20260928`. Credentials,
  TLS keys and runtime configuration stay there and must not be committed.
  Named containers: `ws5-alignment-oidc-dex`, `ws5-alignment-oidc-proxy`.
  Native process tool sessions: Center `57411`, Vite `24832`.

## Passed browser checks

`browser.cjs` session `61015` exited zero; `result.json` contains six checks:

1. Direct unauthenticated Center `/api/v1/auth/me` returns 401.
2. Opening `/controllers` through the OAuth proxy redirects to Dex.
3. Real username/password submission and OAuth consent/callback establish a
   proxy session; Center `/auth/me` returns a subject and `controllers:read`.
4. The original `/controllers` deep link survives the login round trip.
5. Reload retains authenticated access to `/api/v1/controllers`.
6. Clearing browser cookies and reloading returns to Dex even when the
   dashboard's local login flag was set.

Artifacts include `authenticated.png`, `browser.log`, `result.json` and the
browser scripts. Initial setup attempts exposed a local cookie-secret encoding
mistake, a readiness race and an incorrect test URL; those were corrected before
the passing run. They were not Center application failures.

## Open defect: OIDC logout does not terminate the proxy session

The independent `browser-logout.cjs` observation (session `27987`, exit zero)
waits for the actual logout response before refreshing. Its
`logout-observation.json` records:

```json
{
  "logoutEndpointStatus": 404,
  "apiStatusAfterLogoutAndReload": 200,
  "loginFlagAfterLogout": "1",
  "pathAfterLogout": "/"
}
```

`TopBar.handleLogout` always calls `authApi.logout`, which swallows errors and
then clears only the dashboard flag. `add_local_auth_routes` omits logout when
password login is disabled. The HttpOnly OAuth proxy cookie survives, so a
subsequent page load authenticates again. The observation script passing means
the defect was reproduced, not that logout passed.

Required repair evidence: a configured external logout flow must terminate the
proxy session; local password logout must continue to work; failed or unavailable
logout must not be presented as successful. Keep external IdP SSO termination
distinct from the application's proxy session, and account for deployments with
both password and OIDC providers. Do not hard-code a particular external provider
endpoint into the generic dashboard or claim that clearing localStorage logs out.

Kubernetes SAR, deployed owner forwarding, and the original cross-resource traffic
and failure scenarios remain open. This evidence does not close the overall goal.
