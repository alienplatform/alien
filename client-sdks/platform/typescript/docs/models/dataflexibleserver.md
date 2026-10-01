# DataFlexibleServer

Azure Flexible Server backend.

## Example Usage

```typescript
import { DataFlexibleServer } from "@alienplatform/platform-api/models";

let value: DataFlexibleServer = {
  serverName: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "flexibleServer",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `serverName`                                                               | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `state`                                                                    | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus35](../models/resourceheartbeatstatus35.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `version`                                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `backend`                                                                  | *"flexibleServer"*                                                         | :heavy_check_mark:                                                         | N/A                                                                        |