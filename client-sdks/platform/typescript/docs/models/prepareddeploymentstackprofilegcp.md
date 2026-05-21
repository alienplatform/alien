# PreparedDeploymentStackProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { PreparedDeploymentStackProfileGcp } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                | [models.PreparedDeploymentStackProfileGcpBinding](../models/prepareddeploymentstackprofilegcpbinding.md) | :heavy_check_mark:                                                                                       | Generic binding configuration for permissions                                                            |
| `grant`                                                                                                  | [models.PreparedDeploymentStackProfileGcpGrant](../models/prepareddeploymentstackprofilegcpgrant.md)     | :heavy_check_mark:                                                                                       | Grant permissions for a specific cloud platform                                                          |