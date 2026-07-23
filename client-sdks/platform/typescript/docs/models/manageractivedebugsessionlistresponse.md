# ManagerActiveDebugSessionListResponse

Paginated response

## Example Usage

```typescript
import { ManagerActiveDebugSessionListResponse } from "@alienplatform/platform-api/models";

let value: ManagerActiveDebugSessionListResponse = {
  items: [
    {
      id: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      state: "failed",
      expiresAt: new Date("2024-07-14T07:23:56.850Z"),
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `items`                                                                      | [models.ManagerActiveDebugSession](../models/manageractivedebugsession.md)[] | :heavy_check_mark:                                                           | Items in this page                                                           |
| `nextCursor`                                                                 | *string*                                                                     | :heavy_check_mark:                                                           | Cursor for the next page, null if last page                                  |
