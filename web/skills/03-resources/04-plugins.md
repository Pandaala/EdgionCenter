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

## EdgionStreamPlugins (Pending Development)

```yaml
apiVersion: edgion.io/v1
kind: EdgionStreamPlugins
metadata:
  name: my-stream-plugins
  namespace: default
spec:
  plugins:
    - type: IpRestriction
      config:
        ipSource: remoteAddr              # IP source: remoteAddr (connection IP)
        allow:                            # IP allowlist (CIDR format)
          - "10.0.0.0/8"
          - "172.16.0.0/12"
        deny:                             # IP blocklist (higher priority than allow)
          - "10.0.0.100/32"
        defaultAction: allow              # Default action: allow | deny
        message: "Access denied"          # Message on denial
```

**IP filter logic**: deny list match → reject → allow list match → allow → defaultAction

**Route binding** (via annotation):
```yaml
# Same namespace
annotations:
  edgion.io/edgion-stream-plugins: "my-stream-plugins"

# Cross-namespace
annotations:
  edgion.io/edgion-stream-plugins: "other-namespace/my-stream-plugins"
```

**Supported protocols**: Gateway listener-level connection filtering, TCPRoute, TLSRoute

**Development Notes**:
- Namespaced resource, kind must be added to ResourceKind: `edgionstreamplugins`
- Simpler than EdgionPlugins — **no four-phase pipeline**, just a single plugins list
- Currently only one plugin type: IpRestriction
- Form: metadata + plugins list editing
  - type selection (currently only IpRestriction)
  - config editing (ipSource, allow, deny, defaultAction, message)
- List page displays: name, namespace, plugin count, plugin type list
- IP check runs at connection establishment time, with minimal performance impact
- Plugin configuration supports hot reload

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
