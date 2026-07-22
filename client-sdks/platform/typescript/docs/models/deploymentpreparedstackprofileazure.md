# DeploymentPreparedStackProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentPreparedStackProfileAzure } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                    | [models.DeploymentPreparedStackProfileAzureBinding](../models/deploymentpreparedstackprofileazurebinding.md) | :heavy_check_mark:                                                                                           | Generic binding configuration for permissions                                                                |
| `description`                                                                                                | *string*                                                                                                     | :heavy_minus_sign:                                                                                           | Short admin-facing description of why this entry exists.                                                     |
| `grant`                                                                                                      | [models.DeploymentPreparedStackProfileAzureGrant](../models/deploymentpreparedstackprofileazuregrant.md)     | :heavy_check_mark:                                                                                           | Grant permissions for a specific cloud platform                                                              |
| `label`                                                                                                      | *string*                                                                                                     | :heavy_minus_sign:                                                                                           | Stable admin-facing label for this permission entry.                                                         |
