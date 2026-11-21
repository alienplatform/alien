# KubernetesExposureCustomBaseDomainCertificateManagedTLSSecret

## Example Usage

```typescript
import { KubernetesExposureCustomBaseDomainCertificateManagedTLSSecret } from "@alienplatform/platform-api/models";

let value: KubernetesExposureCustomBaseDomainCertificateManagedTLSSecret = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `mode`                                                                   | *"managedTlsSecret"*                                                     | :heavy_check_mark:                                                       | N/A                                                                      |
| `secretNameTemplate`                                                     | *string*                                                                 | :heavy_check_mark:                                                       | Secret name template. Runtime may substitute resource/deployment tokens. |