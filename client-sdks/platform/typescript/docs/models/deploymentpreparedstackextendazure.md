# DeploymentPreparedStackExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentPreparedStackExtendAzure } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                  | [models.DeploymentPreparedStackExtendAzureBinding](../models/deploymentpreparedstackextendazurebinding.md) | :heavy_check_mark:                                                                                         | Generic binding configuration for permissions                                                              |
| `description`                                                                                              | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Short admin-facing description of why this entry exists.                                                   |
| `grant`                                                                                                    | [models.DeploymentPreparedStackExtendAzureGrant](../models/deploymentpreparedstackextendazuregrant.md)     | :heavy_check_mark:                                                                                         | Grant permissions for a specific cloud platform                                                            |
| `label`                                                                                                    | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Stable admin-facing label for this permission entry.                                                       |
