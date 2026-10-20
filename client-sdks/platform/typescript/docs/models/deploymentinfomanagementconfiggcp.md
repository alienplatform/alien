# DeploymentInfoManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { DeploymentInfoManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: DeploymentInfoManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `federationTrust`                                                  | *models.DeploymentInfoFederationTrustUnion*                        | :heavy_minus_sign:                                                 | N/A                                                                |
| `serviceAccountEmail`                                              | *string*                                                           | :heavy_check_mark:                                                 | Linked source account metadata; never a direct target trust grant. |
| `platform`                                                         | [models.TargetsPlatformGcp](../models/targetsplatformgcp.md)       | :heavy_check_mark:                                                 | N/A                                                                |