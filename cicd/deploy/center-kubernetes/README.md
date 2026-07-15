# Kubernetes Center deployment

The Admin dashboard is fronted by an `oauth2-proxy` sidecar. The proxy performs
the browser authorization-code flow, keeps the browser session in a secure
cookie, and forwards its OIDC ID token as `Authorization: Bearer ...` to Center.
Center remains the resource server and validates the token issuer, audience,
signature, expiry, subject, and groups before running Kubernetes SAR.

Before applying the Kustomization:

1. Register an OIDC confidential web client with callback
   `https://center.example.invalid/oauth2/callback` (replace the example host).
2. Set `oidc-issuer-url`, `oauth2-redirect-url`, `auth.discovery`, and
   `auth.audiences` in `config.yaml`. The sole configured audience must match
   the OAuth client ID. Configure the provider to include the `groups` claim in
   the ID token requested by this client. When the issuer uses a private CA,
   mount its PEM bundle read-only into the Center container and set
   `auth.ca_file` to that mounted path. Do not disable issuer TLS verification
   in production; the operating-system trust store alone is not sufficient for
   the Center binary's rustls/webpki client.
3. Create the required Secret without committing credentials:

   ```sh
   kubectl -n edgion-system create secret generic edgion-center-browser-oidc \
     --from-literal=client-id=edgion-center-dashboard \
     --from-literal=client-secret='<oidc-client-secret>' \
     --from-literal=cookie-secret='<base64url-encoded-32-byte-secret>'
   ```

   Browser users enter through the external host and are redirected to the
   identity provider by `oauth2-proxy`; no Center-local username or password is
   created in Kubernetes mode.

4. Provision `edgion-center-internal-tls` with `tls.crt`, `tls.key`, and
   `ca.crt`. This must use a CA dedicated to Center replicas, separate from the
   federation CA trusted for Controllers. The certificate must contain the DNS
   SAN `edgion-center-internal.edgion-system.svc`; all replicas use that name
   for TLS verification while connecting directly to the Lease holder Pod IP.
   Replica client certificates must carry exactly the configured
   `spiffe://edgion.io/ns/edgion-system/sa/edgion-center` URI SAN. Never issue
   an internal-forwarding client certificate to a Controller.
5. Expose Service port `12201` through a TLS Ingress/Gateway using the same
   external host as the redirect URL. Do not expose the Pod's loopback Center
   listener directly.

Port `12252` belongs only to replica-to-replica forwarding. The normal
`edgion-center` Service does not expose it; the headless internal Service exists
for certificate identity and discovery. mTLS and Lease/Pod-UID fencing remain
mandatory even when a NetworkPolicy also limits access to Center Pods.

The Pod intentionally stays unready when the Secret is absent or the proxy
cannot initialize. API-only clients may bypass the browser proxy only through a
separately secured internal path and must supply a valid bearer token directly.
