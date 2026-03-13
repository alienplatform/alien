# DeploymentExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentExtendAw } from "@aliendotdev/platform-api/models";

let value: DeploymentExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `binding`                                                                  | [models.DeploymentExtendAwBinding](../models/deploymentextendawbinding.md) | :heavy_check_mark:                                                         | Generic binding configuration for permissions                              |
| `grant`                                                                    | [models.DeploymentExtendAwGrant](../models/deploymentextendawgrant.md)     | :heavy_check_mark:                                                         | Grant permissions for a specific cloud platform                            |