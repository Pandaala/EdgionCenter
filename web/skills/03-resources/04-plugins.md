---
name: plugin-resources
description: Plugin resource development guide — EdgionPlugins/EdgionStreamPlugins/EdgionConfigData (based on feature-04-06 user documentation)
---

# Plugin Resources

## EdgionPlugins

- apiVersion: `edgion.io/v1`
- Kind: `edgionplugins`
- Reference code: `src/pages/Plugins/EdgionPluginsList.tsx`

The catalog exposes all 48 executable HTTP plugin names in the current sibling
Edgion enum. Name and stage coverage does not establish complete nested-field or
runtime behavior coverage; continue checking each configuration against source.

Plugin families:
- **Authentication**: Basic Auth, JWT Auth, Key Auth, HMAC Auth, LDAP Auth, Forward Auth, OpenID Connect, JWE Decrypt, Header Cert Auth
- **Security**: CORS, CSRF, IP Restriction, Request Restriction
- **Traffic Control**: Rate Limit, Rate Limit(Redis), Proxy Rewrite, Response Rewrite, Bandwidth Limit, Request Mirror, Direct Endpoint, Dynamic Upstream, **Region Route (new)**
- **Observability**: Real IP, Ctx Setter, RequestId, GeoIpLocation, Mock, DSL, Debug Access Log
- **Body and AI processing**: RequestBodyBuffer, JsonSchemaValidation, FormJsonTransform, AiProxy, AiGuard, Guardrail
- **Outbound credentials and variables**: CredentialInjector, WebhookKeyGet
- **Gateway API Filters**: Request Header Modifier, Response Header Modifier, Request Redirect, URL Rewrite

### AI backend routing

The Services menu exposes EdgionBackend provider/credential/model resources.
HTTPRoute backend references select `edgion.io/EdgionBackend` without a port;
the route attaches an EdgionPlugins resource containing request-stage AiProxy
through the ordinary ExtensionRef filter. The AiProxy catalog exposes backend
selection, usage tracking, unknown-model policy, model routes, and token quota.
Read the current sibling Edgion schema for nested field semantics. Credential
values are never edited inline; EdgionBackend stores Secret references.

Plugin stage eligibility is authoritative in Edgion's
`edgion-resources/src/resources/edgion_plugins/validate.rs`. In particular, ExtProc
response handling is configured by its request-stage entry; it is not a directly
configurable upstreamResponsePlugins entry.

## EdgionStreamPlugins

The connection stage (`spec.plugins`) offers IpRestriction,
GlobalConnectionIpRestriction, ConnectionRateLimit, and GeoIpLocation.
The TLS route stage (`spec.tlsRoutePlugins`) offers IpRestriction only.
The current enums live in the sibling Edgion resource crate's
`edgion_stream_plugins/stream_plugins.rs` and `tls_route_plugins.rs`.

Connection IpRestriction uses named IP groups and optional ConfigData references;
it has no HTTP message/status or ipSource fields. TLS-route IpRestriction reuses
the HTTP config, including RemoteIp/DirectPeerIp. Connection GeoIpLocation shares
the HTTP field schema but only permits DirectPeerIp-sourced rules. The form
reuses its HTTP field definition and displays this stage restriction.

```yaml
apiVersion: edgion.io/v1
kind: EdgionStreamPlugins
metadata:
  name: private-connections
  namespace: default
spec:
  plugins:
    - type: IpRestriction
      config:
        allow:
          - name: private-networks
            cidrs: ["10.0.0.0/8", "172.16.0.0/12"]
        defaultAction: deny
  tlsRoutePlugins:
    - type: IpRestriction
      config:
        ipSource: DirectPeerIp
        defaultAction: allow
```

Both arrays and unknown entry/config fields survive Form/YAML edits. Mutation
filtering removes Controller-owned status and compiled matcher fields. Inspect
current Gateway/listener and route annotation rules in Edgion before changing
attachment behavior; this page does not define a separate attachment contract.

## EdgionConfigData ✅ Completed

```yaml
apiVersion: edgion.io/v1
kind: EdgionConfigData
metadata:
  name: region-route-override
  namespace: default
spec:
  data:
    type: RegionRouteOverride
    config:
      regions:
        - name: east
          hashRange: [0, 499]
          backendEndpoint: "127.0.0.1:30001"
          tls: false
```

**Development Notes**:
- **Namespaced resource**, uses `resourceApi`, kind: `edgionconfigdata`
- The hot-swappable data overlay (`spec.data` is a tagged enum: `KeyList`,
  `IpList`, `Selector`, `RegionRouteOverride`, `Misc`); base config stays in
  `EdgionPlugins`, only data lives here
- Page: `src/pages/Plugins/EdgionConfigDataList.tsx`, route `plugins/metadata`
- The one kind the Center default policy lets a federated Center write remotely
- List page does not need a namespace column
