# DeploymentExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentExtendGcp } from "@aliendotdev/platform-api/models";

let value: DeploymentExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `binding`                                                                    | [models.DeploymentExtendGcpBinding](../models/deploymentextendgcpbinding.md) | :heavy_check_mark:                                                           | Generic binding configuration for permissions                                |
| `grant`                                                                      | [models.DeploymentExtendGcpGrant](../models/deploymentextendgcpgrant.md)     | :heavy_check_mark:                                                           | Grant permissions for a specific cloud platform                              |