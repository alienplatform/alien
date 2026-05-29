# CertificateManagedAcmImport2

## Example Usage

```typescript
import { CertificateManagedAcmImport2 } from "@alienplatform/platform-api/models/operations";

let value: CertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `mode`                                                      | *"managedAcmImport"*                                        | :heavy_check_mark:                                          | N/A                                                         |
| `region`                                                    | *string*                                                    | :heavy_minus_sign:                                          | ACM region. Defaults to the deployment region when omitted. |
| `tags`                                                      | Record<string, *string*>                                    | :heavy_minus_sign:                                          | Tags applied to runtime-imported ACM certificates.          |