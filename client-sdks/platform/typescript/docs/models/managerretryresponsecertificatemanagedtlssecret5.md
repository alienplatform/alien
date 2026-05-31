# ManagerRetryResponseCertificateManagedTLSSecret5

## Example Usage

```typescript
import { ManagerRetryResponseCertificateManagedTLSSecret5 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCertificateManagedTLSSecret5 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `mode`                                                                   | *"managedTlsSecret"*                                                     | :heavy_check_mark:                                                       | N/A                                                                      |
| `secretNameTemplate`                                                     | *string*                                                                 | :heavy_check_mark:                                                       | Secret name template. Runtime may substitute resource/deployment tokens. |