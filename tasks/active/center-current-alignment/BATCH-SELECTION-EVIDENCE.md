# Batch selection, result counts and Controller navigation

## Findings and repair

Nine lists built their delete payload from filtered rows even while the UI retained
hidden selections: HTTP/GRPC/TCP/UDP/TLSRoute, Gateway, EdgionTls, EdgionPlugins and
EdgionStreamPlugins. All now resolve selection against the complete loaded source
list, retaining each selected resourceVersion for CAS deletion.

Thirteen active lists reported successful deletion using current selectedRowKeys,
which could change during the request. Their success count now uses the submitted
mutation variables. The same nine lists plus Service, EndpointSlice,
BackendTLSPolicy and LinkSys are covered. Existing partial-failure handling remains.

ControllerProxy previously reused page state when only controllerId changed. Its
shell is now keyed by normalized Controller identity, clearing selection and editor
drafts on Controller navigation. Captured request targets remain necessary and
unchanged; this is a UI-state boundary, not a replacement for authorization/CAS.

## Focused verification

19 tests pass in four files: thirteen filter/selection/result-count regressions,
one Controller-switch regression, four existing selected-set/partial-failure tests
and the existing LinkSys batch test. The new matrix verifies the submitted complete
set and resourceVersions, then changes selection while deletion is pending and
checks the count still reports the two submitted resources.

Two native browser checkpoints pass on Center 12201 and Vite 15173:

1. Select two actual HTTPRoutes, search until only one is visible, and observe a
   two-resource confirmation. This uses a browser-only delete-permission fixture
   to open the confirmation; it is cancelled and no deletion request is sent.
2. Select Gateway A, open its detail, then navigate within the SPA to Controller B,
   which has the same Gateway namespace/name. The old dialog closes and selection
   clears. B's actual listener port 18210 and enabled refresh are verified.

No resource mutations or policy changes occurred. The first checkpoint is UI
selection evidence, not backend delete authorization or actual deletion proof.
Artifacts: `/tmp/ws5-center-batch-selection-20260928/proof.cjs`, `result.json`,
`filtered-confirm.png`, `switched.png`. Logs:
`/tmp/ws5-center-batch-selection-tests.log`,
`/tmp/ws5-center-batch-selection-full.log`,
`/tmp/ws5-center-batch-selection-build.log`,
`/tmp/ws5-center-batch-selection-lint.log`.

The full frontend suite passes 751 tests in 113 files. Production build and lint
pass; the existing bundle-size warning remains. Browser screenshots were inspected.

No Edgion files changed in this pass. The broader alignment audit remains active.

## Subsequent actual deletion evidence

The repository browser suite now also selects two isolated HTTPRoutes, hides one
with search, confirms deletion, verifies both objects are absent through actual
API reads, and verifies an unselected third object remains. This passed in the
combined native regression; see
[COMBINED-BROWSER-EVIDENCE.md](COMBINED-BROWSER-EVIDENCE.md). It adds authorized
mutation evidence without changing the limits of the earlier two UI checkpoints.
