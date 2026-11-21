# ExternalBindingUnion8

What an installer supplies for an AI resource with an external binding.

Two inputs, told apart by whether the JSON carries a `service` tag (other than `external-ai`):
- `{ "provider", "apiKey" }` — BYO key (the original input; stored deployments and Helm values
  carry it, so it parses exactly as before). `{ "service": "external-ai", "provider",
  "apiKey" }` also parses, as it did before the cloud-identity form existed.
- `{ "service": "bedrock", "region" }` — the cloud's own AI service, called with the workload's
  ambient identity (for example an EKS pod's IRSA role). No key, no Secret.

The cloud-identity form needs the pod to have its own AWS identity: on Kubernetes, an IRSA role
on the service account (or an EKS Pod Identity association) with `bedrock:InvokeModel` and
`bedrock:InvokeModelWithResponseStream`. Without one the gateway refuses to sign and never uses
the node's role. The `region` must be a plain AWS region name such as `us-east-1`.

Access is not checked at install time (the setup credentials cannot speak for the pod's role):
it is checked on the first call, and a model the role cannot use fails with
`provider_access_unavailable`.

Untagged, because the BYO-key input has no tag and must keep parsing unchanged. The two shapes
cannot overlap: the ambient one requires `service`, the key one requires `provider` and `apiKey`.
`Deserialize` is hand-written (see below) so a typo gets an error naming both accepted forms, and
so an ambient input carrying an unused key such as `apiKey` is refused instead of dropped.


## Supported Types

### `models.ExternalBindingBedrock`

```typescript
const value: models.ExternalBindingBedrock = {
  region: "<value>",
  service: "bedrock",
  type: "ai",
};
```

### `models.ExternalBindingAi`

```typescript
const value: models.ExternalBindingAi = {
  apiKey: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  provider: "<value>",
  type: "ai",
};
```
