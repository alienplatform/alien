# DataLocal11

## Example Usage

```typescript
import { DataLocal11 } from "@alienplatform/platform-api/models";

let value: DataLocal11 = {
  reachable: false,
  registryUrl: "https://well-documented-remark.biz/",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `reachable`                                                                | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `registryUrl`                                                              | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus55](../models/resourceheartbeatstatus55.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"local"*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |