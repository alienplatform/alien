# DeploymentProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentProfileAw } from "@alienplatform/platform-api/models";

let value: DeploymentProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `binding`                                                                    | [models.DeploymentProfileAwBinding](../models/deploymentprofileawbinding.md) | :heavy_check_mark:                                                           | Generic binding configuration for permissions                                |
| `grant`                                                                      | [models.DeploymentProfileAwGrant](../models/deploymentprofileawgrant.md)     | :heavy_check_mark:                                                           | Grant permissions for a specific cloud platform                              |