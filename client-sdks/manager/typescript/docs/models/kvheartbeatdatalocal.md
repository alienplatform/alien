# KvHeartbeatDataLocal

## Example Usage

```typescript
import { KvHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: KvHeartbeatDataLocal = {
  cloudMetadataSupported: true,
  name: "<value>",
  path: "/etc",
  pathExists: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `cloudMetadataSupported`                                   | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `isDirectory`                                              | *boolean*                                                  | :heavy_minus_sign:                                         | N/A                                                        |
| `name`                                                     | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `path`                                                     | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `pathExists`                                               | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `status`                                                   | [models.KvHeartbeatStatus](../models/kvheartbeatstatus.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"local"*                                                  | :heavy_check_mark:                                         | N/A                                                        |