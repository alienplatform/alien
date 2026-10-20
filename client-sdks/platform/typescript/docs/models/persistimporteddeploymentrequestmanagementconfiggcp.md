# PersistImportedDeploymentRequestManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { PersistImportedDeploymentRequestManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `federationTrust`                                                  | *models.PersistImportedDeploymentRequestFederationTrustUnion*      | :heavy_minus_sign:                                                 | N/A                                                                |
| `serviceAccountEmail`                                              | *string*                                                           | :heavy_check_mark:                                                 | Linked source account metadata; never a direct target trust grant. |
| `platform`                                                         | *"gcp"*                                                            | :heavy_check_mark:                                                 | N/A                                                                |