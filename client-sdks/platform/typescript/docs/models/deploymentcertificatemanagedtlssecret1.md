# DeploymentCertificateManagedTLSSecret1

## Example Usage

```typescript
import { DeploymentCertificateManagedTLSSecret1 } from "@alienplatform/platform-api/models";

let value: DeploymentCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `mode`                                                                   | *"managedTlsSecret"*                                                     | :heavy_check_mark:                                                       | N/A                                                                      |
| `secretNameTemplate`                                                     | *string*                                                                 | :heavy_check_mark:                                                       | Secret name template. Runtime may substitute resource/deployment tokens. |