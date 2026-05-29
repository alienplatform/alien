# BuildHeartbeatDataAzureContainerApps

## Example Usage

```typescript
import { BuildHeartbeatDataAzureContainerApps } from "@alienplatform/manager-api/models";

let value: BuildHeartbeatDataAzureContainerApps = {
  environmentVariableCount: 180128,
  events: [],
  managedEnvironmentId: "<id>",
  resourceGroupName: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "azureContainerApps",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `environmentVariableCount`                                       | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `events`                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]           | :heavy_check_mark:                                               | N/A                                                              |
| `managedEnvironmentId`                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `managedIdentityId`                                              | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `resourceGroupName`                                              | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `resourcePrefix`                                                 | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [models.BuildHeartbeatStatus](../models/buildheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"azureContainerApps"*                                           | :heavy_check_mark:                                               | N/A                                                              |