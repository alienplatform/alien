# KubernetesTlsSecretRef

Namespace-scoped Kubernetes TLS Secret reference.

## Example Usage

```typescript
import { KubernetesTlsSecretRef } from "@alienplatform/manager-api/models";

let value: KubernetesTlsSecretRef = {
  secretName: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `namespace`                                                       | *string*                                                          | :heavy_minus_sign:                                                | Secret namespace. Defaults to the release namespace when omitted. |
| `secretName`                                                      | *string*                                                          | :heavy_check_mark:                                                | Secret name.                                                      |