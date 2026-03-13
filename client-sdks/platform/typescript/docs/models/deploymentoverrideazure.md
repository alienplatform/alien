# DeploymentOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentOverrideAzure } from "@aliendotdev/platform-api/models";

let value: DeploymentOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `binding`                                                                            | [models.DeploymentOverrideAzureBinding](../models/deploymentoverrideazurebinding.md) | :heavy_check_mark:                                                                   | Generic binding configuration for permissions                                        |
| `grant`                                                                              | [models.DeploymentOverrideAzureGrant](../models/deploymentoverrideazuregrant.md)     | :heavy_check_mark:                                                                   | Grant permissions for a specific cloud platform                                      |