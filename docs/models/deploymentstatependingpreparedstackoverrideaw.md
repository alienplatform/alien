# DeploymentStatePendingPreparedStackOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackOverrideAw } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.DeploymentStatePendingPreparedStackOverrideAwBinding](../models/deploymentstatependingpreparedstackoverrideawbinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `description`                                                                                                                    | *string*                                                                                                                         | :heavy_minus_sign:                                                                                                               | Short admin-facing description of why this entry exists.                                                                         |
| `effect`                                                                                                                         | [models.DeploymentStatePendingPreparedStackOverrideEffect](../models/deploymentstatependingpreparedstackoverrideeffect.md)       | :heavy_minus_sign:                                                                                                               | IAM effect. Defaults to Allow.                                                                                                   |
| `grant`                                                                                                                          | [models.DeploymentStatePendingPreparedStackOverrideAwGrant](../models/deploymentstatependingpreparedstackoverrideawgrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |
| `label`                                                                                                                          | *string*                                                                                                                         | :heavy_minus_sign:                                                                                                               | Stable admin-facing label for this permission entry.                                                                             |