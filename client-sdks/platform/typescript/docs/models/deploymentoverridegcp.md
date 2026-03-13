# DeploymentOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentOverrideGcp } from "@aliendotdev/platform-api/models";

let value: DeploymentOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `binding`                                                                        | [models.DeploymentOverrideGcpBinding](../models/deploymentoverridegcpbinding.md) | :heavy_check_mark:                                                               | Generic binding configuration for permissions                                    |
| `grant`                                                                          | [models.DeploymentOverrideGcpGrant](../models/deploymentoverridegcpgrant.md)     | :heavy_check_mark:                                                               | Grant permissions for a specific cloud platform                                  |