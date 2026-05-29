# BuildHeartbeatDataGcpCloudBuild

## Example Usage

```typescript
import { BuildHeartbeatDataGcpCloudBuild } from "@alienplatform/manager-api/models";

let value: BuildHeartbeatDataGcpCloudBuild = {
  buildConfigId: "<id>",
  environmentVariableCount: 770056,
  events: [],
  location: "<value>",
  projectId: "<id>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "gcpCloudBuild",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `buildConfigId`                                                  | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `environmentVariableCount`                                       | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `events`                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]           | :heavy_check_mark:                                               | N/A                                                              |
| `location`                                                       | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `projectId`                                                      | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `serviceAccount`                                                 | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [models.BuildHeartbeatStatus](../models/buildheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"gcpCloudBuild"*                                                | :heavy_check_mark:                                               | N/A                                                              |