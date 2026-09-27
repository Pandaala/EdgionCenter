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
  Initial native process tool sessions: Center `57411`, Vite `24832`.
  The repair section below records the replacement Center session.

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

## Original defect: OIDC logout did not terminate the proxy session

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

At this checkpoint, `TopBar.handleLogout` always called `authApi.logout`, which
swallowed errors and then cleared only the dashboard flag. Password-disabled
compositions omit the local logout endpoint. The HttpOnly OAuth proxy cookie
survived, so a subsequent page load authenticated again. The observation script passing means
the defect was reproduced, not that logout passed.

Required repair evidence: a configured external logout flow must terminate the
proxy session; local password logout must continue to work; failed or unavailable
logout must not be presented as successful. Keep external IdP SSO termination
distinct from the application's proxy session, and account for deployments with
both password and OIDC providers. Do not hard-code a particular external provider
endpoint into the generic dashboard or claim that clearing localStorage logs out.

Kubernetes SAR, deployed owner forwarding, and the original cross-resource traffic
and failure scenarios remain open. This evidence does not close the overall goal.

## Repair and current evidence

- Added optional `auth.logout_path` with same-origin path validation; `/auth/me`
  reports the actual authenticated provider, its logout path and availability
  of local cookie logout. The top bar uses this identity rather than guessing
  from deployment capabilities. Missing configuration and failed API requests
  produce explicit feedback; they do not clear the flag and claim success.
- For mixed authentication, clear the Center password cookie before navigating
  to the external proxy logout path. The login page checks an existing session
  before presenting password fields, so an authenticated OIDC request is accepted
  even when password login is also offered.
- Session `11580` passed seven real browser checks on the OIDC-only runtime,
  including proxy-cookie deletion and `/oauth2/auth` returning 401 after logout.
  Session `3210` repeated those checks on the mixed runtime and passed nine
  checks with a preexisting Center password cookie: after OIDC logout, that
  cookie also no longer authenticates directly to Center (401).
- The repository authentication setup and shell logout case passed via both
  password and OIDC entry points against the final dashboard (session `45336`,
  two tests per mode). The project named `kubernetes` was deliberately pointed
  at the native Dex/proxy runtime; no deployed Kubernetes claim is made.
- Focused frontend auth checks: 16 passed. Final frontend suite: 102 files,
  635 tests. Build/lint, E2E types and inventory passed. App tests: 283 default,
  244 without default features. Workspace matrix formatting, Clippy, tests,
  dependency purity and manifest rendering passed; its final exit 1 comes from
  the preexisting `fix-issue-workflow-generic.zh.md` English-only guard. The
  following no-legacy guard was run separately and passed.
- Logs: `/tmp/ws5-center-logout-{matrix,rust-v2,rust-no-default}.log`,
  `/tmp/ws5-center-logout-{web-full-final,build-final,lint-final}.log`,
  `/tmp/ws5-center-logout-{e2e-types,inventory,legacy}.log`.
  Browser logs/results and `repo-final-{standalone,kubernetes}` reports remain
  in the private artifact directory above. No credentials were committed.
- The owned mixed runtime is retained: Center session `47666`, Vite `24832`,
  the same two Docker containers; config `center-mixed.yaml` in that directory.
  Earlier Center sessions `57411` and `53551` were stopped intentionally.

The logout defect is repaired. The wider alignment goal remains active.
