# DataGcpCloudBuild

## Example Usage

```typescript
import { DataGcpCloudBuild } from "@alienplatform/platform-api/models/operations";

let value: DataGcpCloudBuild = {
  buildConfigId: "<id>",
  environmentVariableCount: 982514,
  location: "<value>",
  projectId: "<id>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  backend: "gcpCloudBuild",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `buildConfigId`                                                    | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `environmentVariableCount`                                         | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `location`                                                         | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `projectId`                                                        | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `serviceAccount`                                                   | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus51](../../models/operations/datastatus51.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"gcpCloudBuild"*                                                  | :heavy_check_mark:                                                 | N/A                                                                |