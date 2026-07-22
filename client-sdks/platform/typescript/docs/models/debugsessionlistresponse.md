# DebugSessionListResponse

Paginated response

## Example Usage

```typescript
import { DebugSessionListResponse } from "@alienplatform/platform-api/models";

let value: DebugSessionListResponse = {
  items: [
    {
      id: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
      state: "stopped",
      mode: "push",
      presignedUrls: {},
      createdAt: new Date("2025-10-15T12:13:12.531Z"),
      expiresAt: new Date("2026-01-18T14:49:27.434Z"),
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      deployment: {
        id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
        name: "<value>",
        deploymentGroup: {
          id: "dg_r27ict8c7vcgsumpj90ackf7b",
          name: "<value>",
        },
      },
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `items`                                            | [models.DebugSession](../models/debugsession.md)[] | :heavy_check_mark:                                 | Items in this page                                 |
| `nextCursor`                                       | *string*                                           | :heavy_check_mark:                                 | Cursor for the next page, null if last page        |
