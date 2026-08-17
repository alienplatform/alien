# DeploymentStatePreparedStackOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentStatePreparedStackOverrideAw } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                          | [models.DeploymentStatePreparedStackOverrideAwBinding](../models/deploymentstatepreparedstackoverrideawbinding.md) | :heavy_check_mark:                                                                                                 | Generic binding configuration for permissions                                                                      |
| `description`                                                                                                      | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Short admin-facing description of why this entry exists.                                                           |
| `effect`                                                                                                           | [models.DeploymentStatePreparedStackOverrideEffect](../models/deploymentstatepreparedstackoverrideeffect.md)       | :heavy_minus_sign:                                                                                                 | IAM effect. Defaults to Allow.                                                                                     |
| `grant`                                                                                                            | [models.DeploymentStatePreparedStackOverrideAwGrant](../models/deploymentstatepreparedstackoverrideawgrant.md)     | :heavy_check_mark:                                                                                                 | Grant permissions for a specific cloud platform                                                                    |
| `label`                                                                                                            | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Stable admin-facing label for this permission entry.                                                               |