# DeploymentDetailResponseOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseOverrideAw } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                  | [models.DeploymentDetailResponseOverrideAwBinding](../models/deploymentdetailresponseoverrideawbinding.md) | :heavy_check_mark:                                                                                         | Generic binding configuration for permissions                                                              |
| `effect`                                                                                                   | [models.DeploymentDetailResponseOverrideEffect](../models/deploymentdetailresponseoverrideeffect.md)       | :heavy_minus_sign:                                                                                         | IAM effect. Defaults to Allow.                                                                             |
| `grant`                                                                                                    | [models.DeploymentDetailResponseOverrideAwGrant](../models/deploymentdetailresponseoverrideawgrant.md)     | :heavy_check_mark:                                                                                         | Grant permissions for a specific cloud platform                                                            |