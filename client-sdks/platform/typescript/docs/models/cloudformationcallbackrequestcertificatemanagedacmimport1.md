# CloudFormationCallbackRequestCertificateManagedAcmImport1

## Example Usage

```typescript
import { CloudFormationCallbackRequestCertificateManagedAcmImport1 } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `mode`                                                      | *"managedAcmImport"*                                        | :heavy_check_mark:                                          | N/A                                                         |
| `region`                                                    | *string*                                                    | :heavy_minus_sign:                                          | ACM region. Defaults to the deployment region when omitted. |
| `tags`                                                      | Record<string, *string*>                                    | :heavy_minus_sign:                                          | Tags applied to runtime-imported ACM certificates.          |