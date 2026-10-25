# Integrate WAF support diagnostics into EdgionCenter

## Meta

| Key | Value |
|-----|-------|
| Created | 2026-09-24 |
| Status | todo |
| Type | feature |
| Priority | P2 |
| Issue | N/A |

## AI guide summary

Add an authenticated, read-only WAF support inspection flow for an `EdgionPlugins` draft in the selected Controller context. Reuse Edgion's canonical `WafConfig::inspect_support()` and `WafSupportReport`; Center must not maintain a separate SecLang parser or diagnostics policy. The main design work is making the shared implementation available to both Center compositions while keeping input, output, and authorization boundaries safe.

## Existing Edgion contract

- `Edgion/edgion-resources/src/resources/edgion_plugins/plugin_configs/waf.rs` exposes `WafConfig::inspect_support() -> WafSupportReport`. The report is versioned, bounded, and value-free. It separates `analysisComplete` from `strictValidationPassed`, includes aggregate counts, findings, omitted counts, and a safe failure category, reason, and numeric location.
- `Edgion/edgion-cli/src/cli/commands/waf.rs` provides `edgion-cli waf inspect --file <EdgionPlugins.yaml> --format json|text` as a local reference. It limits input to 16 MiB. The CLI command is not a Center API.
- Inspection never publishes configuration. A successful report is not an apply, admission, Controller status, or Gateway runtime decision.

## Scope

1. Choose a shared-code dependency and release strategy that gives both `edgion-center-standalone` and `edgion-center-kubernetes` access to the same Edgion diagnostics implementation. Preserve their existing SQL and Kubernetes dependency boundaries. Coordinate any necessary shared API or schema change in the Edgion repository.
2. Add an authenticated, read-only Center endpoint for inspecting the current `EdgionPlugins.spec.waf` draft without submitting it to a Controller. Place it in the selected Controller resource context, with the appropriate authorization and audit behavior for each Center composition. Do not add `EdgionPlugins` to the federated `GlobalResources` inventory.
3. Expose the report in the `EdgionPlugins` editor, including the form and YAML editing paths. Show incomplete analysis, strict validation failure, unavailable capabilities, and truncated findings distinctly. Preserve the draft and require a separate explicit apply action for publication.
4. Bound request bytes and diagnostic CPU/memory work. Use at most the CLI's 16 MiB document limit unless a smaller Center limit is justified. Return safe errors for malformed YAML, absent `spec.waf`, oversized input, and inspection failures. Never return or log raw rules, operands, source excerpts, file or asset identity, or untrusted parser errors through the API, audit trail, or UI.
5. Document the operator workflow and the meaning of the report fields. Keep `reportVersion` visible to API consumers and handle unknown versions explicitly.

## Acceptance

- An authorized operator can inspect an unsaved `EdgionPlugins` WAF draft from the selected Controller view in both Center compositions, with no resource write or Gateway side effect.
- The Center response uses Edgion's canonical `WafSupportReport` semantics. The UI does not equate `analysisComplete` with `strictValidationPassed`, nor present inspection success as publication success.
- Authorization, malformed input, missing WAF configuration, oversized input, unsupported semantics, and truncated findings have focused tests. Sensitive markers in submitted rules never appear in API responses, logs, audit data, or UI diagnostics.
- Both Center binaries and dashboard build; relevant tests pass. Any cross-repository shared API change is verified in Edgion as well.

## Starting points

- `web/src/components/ResourceEditor/EdgionPlugins/EdgionPluginsEditor.tsx`
- `web/src/pages/Plugins/EdgionPluginsList.tsx`
- `skills/01-architecture/06-center/04-global-resource-management.md`
- The Center API and authorization handlers used by the selected Controller resource editor
