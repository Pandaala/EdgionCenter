---
name: dashboard-write-path-no-preflight-access-fetch
description: Use when reviewing dashboard `web/src/api/resources.ts` mutation findings that claim create/update/delete must re-read `/api/v1/access` before sending the write, or that the cached `useControllerAccess` model is an unsafe authorization basis; explains why the client-side pre-flight was deliberately removed.
---

# Dashboard Writes Do Not Pre-Fetch `/api/v1/access` — BY DESIGN

## Conclusion

`resourceApi.{create,update,delete}` and `clusterResourceApi.{create,update,delete}`
(`web/src/api/resources.ts`) issue exactly one request: the write. They deliberately do
**not** fetch `/api/v1/access` first to verify the verb locally.

Fix suggestions of the form "re-read the effective policy immediately before every
mutation because cached UI state is never authority for a write", or "the dashboard can
send a write the Controller will reject — add a pre-flight authorization check", are
**not accepted**. The premise confuses the *presentation* gate with the *enforcement*
point.

A client-side pre-flight existed until 2026-07-27 (`requireResourceMutation`) and was
removed.

## Core Rationale

**1. The Controller enforces the policy on the write itself; the dashboard cannot.**

Every Center-originated write against a Controller resource is an ordinary Controller
Admin API call tunnelled through `HttpProxyRequest`. On the Controller side the fed
router applies `authz_layer` and `inject_center_identity` to that request
(`Edgion/edgion-controller/src/cli/mod.rs:738-762`), and the proxied request has its
`Authorization`/`Cookie` stripped — asserted by the Controller-side test
`http_proxy_forwards_if_match_and_strips_authorization`
(`Edgion/edgion-controller/src/fed_sync/fed_client/mod.rs:1054`). A browser-side check
therefore cannot grant, deny, or tighten anything the Controller does not already
enforce on the write.

**2. The removed check was a duplicate read, not a second enforcement point.**

The pre-flight read the *same* `/api/v1/access` document the page already held via
`useControllerAccess`, then compared verbs in the browser. Re-reading it milliseconds
before the write changes nothing that is enforced; it only doubles the round trips.
`batchDelete` loops the single delete, so N deletions cost 2N tunnelled requests against
the Controller-side proxy concurrency cap.

**3. The cached model is already safe for its actual job — enabling a button.**

`useControllerAccess` (`web/src/hooks/useControllerAccess.ts`) uses a 10 s `staleTime`
with `refetchOnWindowFocus`, and its `accessIsUsable` guard drops `data` to `undefined`
while a background refetch is in flight or after a refetch error — React Query's retained
stale snapshot is never used to authorize. `PermissionAwareButton` fails mutations closed
when access is loading or unavailable. Coverage lives with the gate, in
`web/src/hooks/useControllerAccess.test.ts` and
`web/src/components/resource/PermissionAwareButton.test.ts`.

## Consequences Accepted

- A write attempted outside the button flow (direct API call, or access revoked between
  render and submit) fails with a Controller `403` rather than a local client error. This
  is the correct failure location: the single enforcement point reports the denial.
- `web/src/api/resources.test.ts` asserts `controllerAccessApi.get` is **not** called
  during a create. Reintroducing a pre-flight fetch breaks that test on purpose.

## Re-evaluation Triggers

Re-open this decision only if:

- The Controller stops applying `authz_layer` to the proxied fed path — the enforcement
  point would move, and the whole rationale collapses. (This is also guarded on the
  Controller side: an admin route with no `classify()` arm fails a test and default-denies.)
- A write path is added that the Controller cannot authorize server-side, in which case
  the correct fix is Controller-side authorization, not a browser pre-flight.

## Reference Cases

- `07-remove-per-mutation-access-refetch` (Size S), closed 2026-07-27 by removing
  `requireResourceMutation` and its six call sites.
- Source: `web/src/api/resources.ts` (`mutationRequestConfig`),
  `web/src/hooks/useControllerAccess.ts` (`accessIsUsable`),
  `web/src/components/resource/PermissionAwareButton.tsx` (`resolveActionAvailability`).
