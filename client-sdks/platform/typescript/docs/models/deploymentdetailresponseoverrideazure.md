# DeploymentDetailResponseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseOverrideAzure } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                        | [models.DeploymentDetailResponseOverrideAzureBinding](../models/deploymentdetailresponseoverrideazurebinding.md) | :heavy_check_mark:                                                                                               | Generic binding configuration for permissions                                                                    |
| `grant`                                                                                                          | [models.DeploymentDetailResponseOverrideAzureGrant](../models/deploymentdetailresponseoverrideazuregrant.md)     | :heavy_check_mark:                                                                                               | Grant permissions for a specific cloud platform                                                                  |