# PreparedDeploymentStackExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { PreparedDeploymentStackExtendAzure } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                  | [models.PreparedDeploymentStackExtendAzureBinding](../models/prepareddeploymentstackextendazurebinding.md) | :heavy_check_mark:                                                                                         | Generic binding configuration for permissions                                                              |
| `grant`                                                                                                    | [models.PreparedDeploymentStackExtendAzureGrant](../models/prepareddeploymentstackextendazuregrant.md)     | :heavy_check_mark:                                                                                         | Grant permissions for a specific cloud platform                                                            |