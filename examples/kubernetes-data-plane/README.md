# kubernetes-data-plane

An object service that runs in each customer's Kubernetes cluster and stores data in the object storage that customer already operates (Amazon S3, MinIO, Ceph, or any S3-compatible store). Your backend calls it through the manager's tunnel, so the customer opens nothing inbound.

- `PUT /objects/{key}` streams the body into the bucket. `If-None-Match: *` only creates; `If-Match: <etag>` only replaces that exact version.
- `GET /objects/{key}` streams the object back.
- `GET /objects?prefix=` lists objects.
- `DELETE /objects/{key}` removes an object.

Every request except `/health` needs `Authorization: Bearer <accessToken>`, the token you set per customer when onboarding.

## Release

With the CLI logged in to your manager (`alien login --manager <url> --token <key>`):

```bash
alien release
```

## Onboard a customer

```bash
alien onboard acme --platforms kubernetes --secret-input accessToken=$(openssl rand -hex 24)
```

This prints the `helm install` command and a `values.yaml` for the customer's bucket:

```yaml
infrastructure:
  objects:
    type: storage
    service: s3
    bucketName: acme-objects
    # S3-compatible stores: endpoint and keys. Omit them for Amazon S3 with the pod's AWS identity.
    endpoint: https://minio.acme.internal
    accessKeyId: ...
    secretAccessKey: ...
```

Onboard as many customers as you like; each gets its own deployment and token, and every later `alien release` reaches all of them.

## Call it

```bash
alien tokens create --tunnel   # a token that can only call tunnels

curl -X PUT --data-binary @report.pdf \
  https://manager.example.com/v1/deployments/acme/tunnels/api/objects/reports/q3.pdf \
  -H "Proxy-Authorization: Bearer ax_tunnel_..." \
  -H "Authorization: Bearer <acme's accessToken>"
```

The manager authenticates `Proxy-Authorization` and forwards the request, with `Authorization` untouched, over the Operator's outbound connection to the `api` container. Bodies stream both ways, so large uploads and downloads don't buffer in the manager.

## Logs

The service logs JSON to stdout. The Operator ships it over OpenTelemetry to the manager, which forwards it to your OTLP backend (Datadog, Grafana, Honeycomb, Axiom, Coralogix, …) tagged with `alien.deployment_id`. Without a backend:

```bash
alien logs --deployment acme/acme --since 1h
```
