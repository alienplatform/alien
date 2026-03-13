# ListManagerEventsResponse

Retrieved events.

## Example Usage

```typescript
import { ListManagerEventsResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListManagerEventsResponse = {
  items: [
    {
      id: "event_MtSA24M3pWuAkQYxgZxuRI",
      deploymentId: "ag_pnj2da55wi5sxbdcav9t273je",
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      debugSessionId: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
      data: {
        stack: "<value>",
        type: "BuildingStack",
      },
      state: "started",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      createdAt: new Date("2026-01-05T03:00:56.315Z"),
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
    },
  ],
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `items`                                 | [models.Event](../../models/event.md)[] | :heavy_check_mark:                      | N/A                                     |