# DeploymentOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentOverrideAw } from "@alienplatform/platform-api/models";

let value: DeploymentOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `binding`                                                                      | [models.DeploymentOverrideAwBinding](../models/deploymentoverrideawbinding.md) | :heavy_check_mark:                                                             | Generic binding configuration for permissions                                  |
| `description`                                                                  | *string*                                                                       | :heavy_minus_sign:                                                             | Short admin-facing description of why this entry exists.                       |
| `effect`                                                                       | [models.DeploymentOverrideEffect](../models/deploymentoverrideeffect.md)       | :heavy_minus_sign:                                                             | IAM effect. Defaults to Allow.                                                 |
| `grant`                                                                        | [models.DeploymentOverrideAwGrant](../models/deploymentoverrideawgrant.md)     | :heavy_check_mark:                                                             | Grant permissions for a specific cloud platform                                |
| `label`                                                                        | *string*                                                                       | :heavy_minus_sign:                                                             | Stable admin-facing label for this permission entry.                           |