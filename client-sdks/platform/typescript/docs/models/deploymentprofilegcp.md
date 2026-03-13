# DeploymentProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentProfileGcp } from "@aliendotdev/platform-api/models";

let value: DeploymentProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `binding`                                                                      | [models.DeploymentProfileGcpBinding](../models/deploymentprofilegcpbinding.md) | :heavy_check_mark:                                                             | Generic binding configuration for permissions                                  |
| `grant`                                                                        | [models.DeploymentProfileGcpGrant](../models/deploymentprofilegcpgrant.md)     | :heavy_check_mark:                                                             | Grant permissions for a specific cloud platform                                |