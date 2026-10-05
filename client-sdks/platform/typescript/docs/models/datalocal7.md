# DataLocal7

## Example Usage

```typescript
import { DataLocal7 } from "@alienplatform/platform-api/models";

let value: DataLocal7 = {
  cloudMetadataSupported: false,
  name: "<value>",
  path: "/dev",
  pathExists: false,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cloudMetadataSupported`                                                   | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `isDirectory`                                                              | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `path`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `pathExists`                                                               | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus32](../models/resourceheartbeatstatus32.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"local"*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |