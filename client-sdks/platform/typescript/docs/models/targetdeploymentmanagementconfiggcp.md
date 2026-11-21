# TargetDeploymentManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { TargetDeploymentManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: TargetDeploymentManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `federationTrust`                                                  | *models.TargetDeploymentFederationTrustUnion*                      | :heavy_minus_sign:                                                 | N/A                                                                |
| `serviceAccountEmail`                                              | *string*                                                           | :heavy_check_mark:                                                 | Linked source account metadata; never a direct target trust grant. |
| `platform`                                                         | [models.ConfigPlatformGcp](../models/configplatformgcp.md)         | :heavy_check_mark:                                                 | N/A                                                                |