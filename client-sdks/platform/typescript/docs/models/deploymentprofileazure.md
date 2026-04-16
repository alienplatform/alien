# DeploymentProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentProfileAzure } from "@alienplatform/platform-api/models";

let value: DeploymentProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `binding`                                                                          | [models.DeploymentProfileAzureBinding](../models/deploymentprofileazurebinding.md) | :heavy_check_mark:                                                                 | Generic binding configuration for permissions                                      |
| `grant`                                                                            | [models.DeploymentProfileAzureGrant](../models/deploymentprofileazuregrant.md)     | :heavy_check_mark:                                                                 | Grant permissions for a specific cloud platform                                    |