# DeploymentDetailResponseExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseExtendGcp } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                | [models.DeploymentDetailResponseExtendGcpBinding](../models/deploymentdetailresponseextendgcpbinding.md) | :heavy_check_mark:                                                                                       | Generic binding configuration for permissions                                                            |
| `grant`                                                                                                  | [models.DeploymentDetailResponseExtendGcpGrant](../models/deploymentdetailresponseextendgcpgrant.md)     | :heavy_check_mark:                                                                                       | Grant permissions for a specific cloud platform                                                          |