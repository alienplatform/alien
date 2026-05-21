# PreparedDeploymentStackOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { PreparedDeploymentStackOverrideGcp } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                  | [models.PreparedDeploymentStackOverrideGcpBinding](../models/prepareddeploymentstackoverridegcpbinding.md) | :heavy_check_mark:                                                                                         | Generic binding configuration for permissions                                                              |
| `grant`                                                                                                    | [models.PreparedDeploymentStackOverrideGcpGrant](../models/prepareddeploymentstackoverridegcpgrant.md)     | :heavy_check_mark:                                                                                         | Grant permissions for a specific cloud platform                                                            |