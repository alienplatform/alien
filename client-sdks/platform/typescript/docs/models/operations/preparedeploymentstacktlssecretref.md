# PrepareDeploymentStackTlsSecretRef

Namespace-scoped Kubernetes TLS Secret reference.

## Example Usage

```typescript
import { PrepareDeploymentStackTlsSecretRef } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackTlsSecretRef = {
  secretName: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `namespace`                                                       | *string*                                                          | :heavy_minus_sign:                                                | Secret namespace. Defaults to the release namespace when omitted. |
| `secretName`                                                      | *string*                                                          | :heavy_check_mark:                                                | Secret name.                                                      |