# CreateSetupRegistrationOperationRequestCertificateManagedTLSSecret2

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestCertificateManagedTLSSecret2 } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestCertificateManagedTLSSecret2 =
  {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `mode`                                                                   | *"managedTlsSecret"*                                                     | :heavy_check_mark:                                                       | N/A                                                                      |
| `secretNameTemplate`                                                     | *string*                                                                 | :heavy_check_mark:                                                       | Secret name template. Runtime may substitute resource/deployment tokens. |