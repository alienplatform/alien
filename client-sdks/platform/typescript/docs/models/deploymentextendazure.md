# DeploymentExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentExtendAzure } from "@alienplatform/platform-api/models";

let value: DeploymentExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `binding`                                                                        | [models.DeploymentExtendAzureBinding](../models/deploymentextendazurebinding.md) | :heavy_check_mark:                                                               | Generic binding configuration for permissions                                    |
| `grant`                                                                          | [models.DeploymentExtendAzureGrant](../models/deploymentextendazuregrant.md)     | :heavy_check_mark:                                                               | Grant permissions for a specific cloud platform                                  |