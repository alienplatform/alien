# TargetDeploymentProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { TargetDeploymentProfileAw } from "@alienplatform/platform-api/models";

let value: TargetDeploymentProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `binding`                                                                                | [models.TargetDeploymentProfileAwBinding](../models/targetdeploymentprofileawbinding.md) | :heavy_check_mark:                                                                       | Generic binding configuration for permissions                                            |
| `description`                                                                            | *string*                                                                                 | :heavy_minus_sign:                                                                       | Short admin-facing description of why this entry exists.                                 |
| `effect`                                                                                 | [models.TargetDeploymentProfileEffect](../models/targetdeploymentprofileeffect.md)       | :heavy_minus_sign:                                                                       | IAM effect. Defaults to Allow.                                                           |
| `grant`                                                                                  | [models.TargetDeploymentProfileAwGrant](../models/targetdeploymentprofileawgrant.md)     | :heavy_check_mark:                                                                       | Grant permissions for a specific cloud platform                                          |
| `label`                                                                                  | *string*                                                                                 | :heavy_minus_sign:                                                                       | Stable admin-facing label for this permission entry.                                     |