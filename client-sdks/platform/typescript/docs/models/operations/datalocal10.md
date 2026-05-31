# DataLocal10

## Example Usage

```typescript
import { DataLocal10 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal10 = {
  reachable: true,
  registryUrl: "https://white-doorpost.biz/",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `reachable`                                                        | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `registryUrl`                                                      | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus49](../../models/operations/datastatus49.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"local"*                                                          | :heavy_check_mark:                                                 | N/A                                                                |