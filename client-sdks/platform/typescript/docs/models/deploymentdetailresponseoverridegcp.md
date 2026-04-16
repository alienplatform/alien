# DeploymentDetailResponseOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseOverrideGcp } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                    | [models.DeploymentDetailResponseOverrideGcpBinding](../models/deploymentdetailresponseoverridegcpbinding.md) | :heavy_check_mark:                                                                                           | Generic binding configuration for permissions                                                                |
| `grant`                                                                                                      | [models.DeploymentDetailResponseOverrideGcpGrant](../models/deploymentdetailresponseoverridegcpgrant.md)     | :heavy_check_mark:                                                                                           | Grant permissions for a specific cloud platform                                                              |