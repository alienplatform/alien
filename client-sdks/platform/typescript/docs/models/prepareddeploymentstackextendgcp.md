# PreparedDeploymentStackExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { PreparedDeploymentStackExtendGcp } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                              | [models.PreparedDeploymentStackExtendGcpBinding](../models/prepareddeploymentstackextendgcpbinding.md) | :heavy_check_mark:                                                                                     | Generic binding configuration for permissions                                                          |
| `grant`                                                                                                | [models.PreparedDeploymentStackExtendGcpGrant](../models/prepareddeploymentstackextendgcpgrant.md)     | :heavy_check_mark:                                                                                     | Grant permissions for a specific cloud platform                                                        |