# KubernetesCertificateModeManagedTLSSecret

Platform-managed cert written to a Kubernetes TLS Secret.

## Example Usage

```typescript
import { KubernetesCertificateModeManagedTLSSecret } from "@alienplatform/manager-api/models";

let value: KubernetesCertificateModeManagedTLSSecret = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `mode`                                                                   | *"managedTlsSecret"*                                                     | :heavy_check_mark:                                                       | N/A                                                                      |
| `secretNameTemplate`                                                     | *string*                                                                 | :heavy_check_mark:                                                       | Secret name template. Runtime may substitute resource/deployment tokens. |