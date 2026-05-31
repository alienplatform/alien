# DataLocal6

## Example Usage

```typescript
import { DataLocal6 } from "@alienplatform/platform-api/models";

let value: DataLocal6 = {
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `name`                                                     | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `path`                                                     | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `serviceStatus`                                            | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus26](../models/heartbeatstatus26.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"local"*                                                  | :heavy_check_mark:                                         | N/A                                                        |