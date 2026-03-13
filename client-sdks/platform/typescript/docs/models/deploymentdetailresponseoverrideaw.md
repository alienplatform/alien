# DeploymentDetailResponseOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseOverrideAw } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                  | [models.DeploymentDetailResponseOverrideAwBinding](../models/deploymentdetailresponseoverrideawbinding.md) | :heavy_check_mark:                                                                                         | Generic binding configuration for permissions                                                              |
| `grant`                                                                                                    | [models.DeploymentDetailResponseOverrideAwGrant](../models/deploymentdetailresponseoverrideawgrant.md)     | :heavy_check_mark:                                                                                         | Grant permissions for a specific cloud platform                                                            |