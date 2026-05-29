# KubernetesCertificateModeTLSSecretRef

Customer-provided Kubernetes TLS Secret.

## Example Usage

```typescript
import { KubernetesCertificateModeTLSSecretRef } from "@alienplatform/manager-api/models";

let value: KubernetesCertificateModeTLSSecretRef = {
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