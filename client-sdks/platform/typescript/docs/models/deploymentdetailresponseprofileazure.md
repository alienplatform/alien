# DeploymentDetailResponseProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseProfileAzure } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                      | [models.DeploymentDetailResponseProfileAzureBinding](../models/deploymentdetailresponseprofileazurebinding.md) | :heavy_check_mark:                                                                                             | Generic binding configuration for permissions                                                                  |
| `description`                                                                                                  | *string*                                                                                                       | :heavy_minus_sign:                                                                                             | Short admin-facing description of why this entry exists.                                                       |
| `grant`                                                                                                        | [models.DeploymentDetailResponseProfileAzureGrant](../models/deploymentdetailresponseprofileazuregrant.md)     | :heavy_check_mark:                                                                                             | Grant permissions for a specific cloud platform                                                                |
| `label`                                                                                                        | *string*                                                                                                       | :heavy_minus_sign:                                                                                             | Stable admin-facing label for this permission entry.                                                           |