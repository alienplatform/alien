# DeploymentDetailResponseExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseExtendAzure } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                    | [models.DeploymentDetailResponseExtendAzureBinding](../models/deploymentdetailresponseextendazurebinding.md) | :heavy_check_mark:                                                                                           | Generic binding configuration for permissions                                                                |
| `grant`                                                                                                      | [models.DeploymentDetailResponseExtendAzureGrant](../models/deploymentdetailresponseextendazuregrant.md)     | :heavy_check_mark:                                                                                           | Grant permissions for a specific cloud platform                                                              |