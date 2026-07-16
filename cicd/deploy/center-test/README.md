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

When reusing a Kubernetes-focused Controller image that does not contain the
CRD files required by file-system mode, derive the local fixture image from the
sibling Edgion checkout:

```sh
docker build -t edgion-all:local \
  --build-arg CONTROLLER_IMAGE=pandaala/edgion-controller:local \
  -f cicd/deploy/center-test/Dockerfile.controller-local ../Edgion
```

The certificate helper creates short-lived, test-only certificates and replaces
the `center-test-federation-tls` Secret. Do not reuse its CA or keys outside this
fixture. Delete the namespace to remove all generated state.
