# Standalone Center Kubernetes integration fixture

This fixture runs the standalone Center composition and six Edgion Controllers
inside the `edgion-test` namespace. Federation is strict mTLS; plaintext
Controller-to-Center transport is intentionally unsupported.

Build or load the `edgion-center-standalone:local` and `edgion-all:local`
images into the target cluster, then run:

```sh
cicd/deploy/center-test/generate-federation-tls.sh
kubectl apply -f cicd/deploy/center-test/deploy.yaml
```

The certificate helper creates short-lived, test-only certificates and replaces
the `center-test-federation-tls` Secret. Do not reuse its CA or keys outside this
fixture. Delete the namespace to remove all generated state.
