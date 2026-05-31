# DataGcpCloudBuild

## Example Usage

```typescript
import { DataGcpCloudBuild } from "@alienplatform/platform-api/models";

let value: DataGcpCloudBuild = {
  buildConfigId: "<id>",
  environmentVariableCount: 982514,
  location: "<value>",
  projectId: "<id>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "gcpCloudBuild",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `buildConfigId`                                            | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `environmentVariableCount`                                 | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `location`                                                 | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `projectId`                                                | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `serviceAccount`                                           | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus51](../models/heartbeatstatus51.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"gcpCloudBuild"*                                          | :heavy_check_mark:                                         | N/A                                                        |