---
name: dashboard-testing
description: Current EdgionCenter dashboard checks, standalone/Kubernetes runtime setup, and browser evidence rules
---

# Dashboard Testing Guide

## Static baseline

Run from `EdgionCenter/web`:

```bash
npm ci
npx tsc --noEmit
npm run lint
npm test -- --run
npx vite build
```

Attribute failures to the current change, the environment, or a recorded
pre-existing baseline. Do not use one successful command as evidence for another
gate.

## Backend modes

Build from the EdgionCenter repository root:

```bash
cargo build -p edgion-center-standalone
cargo build -p edgion-center-kubernetes
```

- Standalone mode retains SQL/local management and runs the Admin API on 12201.
- Kubernetes mode uses native resources and capability-driven management.
- Controller Admin/probe/federation ports are 12101/12100/12151.
- Center Admin/probe/federation ports are 12201/12200/12251.
- The Vite development server runs on 5173 and proxies `/api` to Center 12201.

Use repository-local config and deployment artifacts; never use a hard-coded
developer home path. For Kubernetes tests, use a task-specific namespace and
list it before any cleanup.

## Resource editor verification

Every editor must prove:

- Current operator fields survive YAML-to-form-to-YAML.
- Unknown operator fields survive a narrow form edit.
- Multiple rules, references, plugins, certificates, and endpoints survive.
- Status, generated metadata, parsed/resolved fields, denial markers, and
  redacted values are absent from mutation payloads.
- Accepted alternate API versions are limited to versions the current
  Controller explicitly converts.

## Browser E2E

Prefer repeatable Playwright cases with stable `data-testid` selectors. For each
page exercise navigation, Controller switching, filters, pagination, refresh,
create/view/edit/delete, batch actions, Form/YAML switching, status details,
permission denial, topology links, and resource-specific operations.

Verify actual API, Controller, or Kubernetes state after mutations; a toast is
not an oracle. Poll conditions using generation/observedGeneration with a bounded
deadline instead of fixed sleeps. Retain failure screenshots and traces.

## Environment safety

- Treat all pre-existing namespaces and cluster-scoped resources as user-owned.
- Give task fixtures a unique label and name prefix.
- Namespace deletion does not clean GatewayClass or EdgionGatewayConfig; list
  and delete exact labeled cluster-scoped fixtures separately.
- Keep the final validated environment until the user authorizes cleanup.
