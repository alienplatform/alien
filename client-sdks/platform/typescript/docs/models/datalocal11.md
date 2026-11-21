# DataLocal11

## Example Usage

```typescript
import { DataLocal11 } from "@alienplatform/platform-api/models";

let value: DataLocal11 = {
  configured: false,
  identity: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `configured`                                                               | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `identity`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus49](../models/resourceheartbeatstatus49.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"local"*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |