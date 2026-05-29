# KubernetesCertificateModeManagedAcmImport

Platform-managed cert imported into AWS ACM by the runtime.

## Example Usage

```typescript
import { KubernetesCertificateModeManagedAcmImport } from "@alienplatform/manager-api/models";

let value: KubernetesCertificateModeManagedAcmImport = {
  mode: "managedAcmImport",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `mode`                                                      | *"managedAcmImport"*                                        | :heavy_check_mark:                                          | N/A                                                         |
| `region`                                                    | *string*                                                    | :heavy_minus_sign:                                          | ACM region. Defaults to the deployment region when omitted. |
| `tags`                                                      | Record<string, *string*>                                    | :heavy_minus_sign:                                          | Tags applied to runtime-imported ACM certificates.          |