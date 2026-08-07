# DataGcpVertex

## Example Usage

```typescript
import { DataGcpVertex } from "@alienplatform/platform-api/models";

let value: DataGcpVertex = {
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "not-checked",
        availability: "unknown",
        blockers: [],
        clientApis: [],
        publicModelId: "<id>",
      },
    ],
    source: "anthropic",
  },
  location: "<value>",
  project: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: false,
  },
  backend: "gcpVertex",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `availability`                                                             | [models.Availability2](../models/availability2.md)                         | :heavy_check_mark:                                                         | N/A                                                                        |
| `location`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `project`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus67](../models/resourceheartbeatstatus67.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"gcpVertex"*                                                              | :heavy_check_mark:                                                         | N/A                                                                        |