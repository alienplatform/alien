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
| `grant`                                                                        | [models.DeploymentOverrideAwGrant](../models/deploymentoverrideawgrant.md)     | :heavy_check_mark:                                                             | Grant permissions for a specific cloud platform                                |