# DeploymentDetailResponseProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseProfileAw } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                | [models.DeploymentDetailResponseProfileAwBinding](../models/deploymentdetailresponseprofileawbinding.md) | :heavy_check_mark:                                                                                       | Generic binding configuration for permissions                                                            |
| `effect`                                                                                                 | [models.DeploymentDetailResponseProfileEffect](../models/deploymentdetailresponseprofileeffect.md)       | :heavy_minus_sign:                                                                                       | IAM effect. Defaults to Allow.                                                                           |
| `grant`                                                                                                  | [models.DeploymentDetailResponseProfileAwGrant](../models/deploymentdetailresponseprofileawgrant.md)     | :heavy_check_mark:                                                                                       | Grant permissions for a specific cloud platform                                                          |