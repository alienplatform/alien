# CreateManagerResponseCertificateTLSSecretRef6

Namespace-scoped Kubernetes TLS Secret reference.

## Example Usage

```typescript
import { CreateManagerResponseCertificateTLSSecretRef6 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCertificateTLSSecretRef6 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `namespace`                                                       | *string*                                                          | :heavy_minus_sign:                                                | Secret namespace. Defaults to the release namespace when omitted. |
| `secretName`                                                      | *string*                                                          | :heavy_check_mark:                                                | Secret name.                                                      |
| `mode`                                                            | *"tlsSecretRef"*                                                  | :heavy_check_mark:                                                | N/A                                                               |