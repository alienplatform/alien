# DeploymentDetailResponseProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseProfileGcp } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                  | [models.DeploymentDetailResponseProfileGcpBinding](../models/deploymentdetailresponseprofilegcpbinding.md) | :heavy_check_mark:                                                                                         | Generic binding configuration for permissions                                                              |
| `grant`                                                                                                    | [models.DeploymentDetailResponseProfileGcpGrant](../models/deploymentdetailresponseprofilegcpgrant.md)     | :heavy_check_mark:                                                                                         | Grant permissions for a specific cloud platform                                                            |