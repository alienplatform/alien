# DeploymentDetailResponseProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseProfileAzure } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                      | [models.DeploymentDetailResponseProfileAzureBinding](../models/deploymentdetailresponseprofileazurebinding.md) | :heavy_check_mark:                                                                                             | Generic binding configuration for permissions                                                                  |
| `grant`                                                                                                        | [models.DeploymentDetailResponseProfileAzureGrant](../models/deploymentdetailresponseprofileazuregrant.md)     | :heavy_check_mark:                                                                                             | Grant permissions for a specific cloud platform                                                                |