# DataFlexibleServer

Azure Flexible Server backend.

## Example Usage

```typescript
import { DataFlexibleServer } from "@alienplatform/platform-api/models/operations";

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

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `serverName`                                                       | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `state`                                                            | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus35](../../models/operations/datastatus35.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `version`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `backend`                                                          | *"flexibleServer"*                                                 | :heavy_check_mark:                                                 | N/A                                                                |