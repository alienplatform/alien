# ListEventsResponse

Paginated response

## Example Usage

```typescript
import { ListEventsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListEventsResponse = {
  items: [
    {
      id: "event_MtSA24M3pWuAkQYxgZxuRI",
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      debugSessionId: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
      data: {
        agentId: "<id>",
        releaseId: "<id>",
        type: "DeletingAgent",
      },
      state: "none",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      createdAt: new Date("2026-05-23T10:59:53.935Z"),
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `items`                                                                 | [models.EventListItemResponse](../../models/eventlistitemresponse.md)[] | :heavy_check_mark:                                                      | Items in this page                                                      |
| `nextCursor`                                                            | *string*                                                                | :heavy_check_mark:                                                      | Cursor for the next page, null if last page                             |
