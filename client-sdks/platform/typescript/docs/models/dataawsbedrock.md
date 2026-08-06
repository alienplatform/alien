# DataAwsBedrock

## Example Usage

```typescript
import { DataAwsBedrock } from "@alienplatform/platform-api/models";

let value: DataAwsBedrock = {
  availability: {
    catalogRevision: "<value>",
    models: [],
    source: "aws-bedrock",
  },
  region: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "awsBedrock",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `availability`                                                             | [models.Availability1](../models/availability1.md)                         | :heavy_check_mark:                                                         | N/A                                                                        |
| `region`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus66](../models/resourceheartbeatstatus66.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"awsBedrock"*                                                             | :heavy_check_mark:                                                         | N/A                                                                        |