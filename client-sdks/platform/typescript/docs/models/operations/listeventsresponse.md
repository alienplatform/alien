# ListEventsResponse

Retrieved events.

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
  releases: [
    {
      id: "rel_WbhQgksrawSKIpEN0NAssHX9",
      version: "<value>",
      gitMetadata: {
        commitSha: "dc36199b2234c6586ebe05ec94078a895c707e29",
        commitMessage:
          "add method to measure Interaction to Next Paint (INP) (#36490)",
        commitRef: "main",
        commitDate: new Date("2026-03-16T12:00:00Z"),
        dirty: true,
        remoteUrl: "https://github.com/alienplatform/alien",
        commitAuthorName: "John Doe",
        commitAuthorEmail: "john@example.com",
        commitAuthorLogin: "johndoe",
        commitAuthorAvatarUrl: "https://github.com/johndoe.png",
      },
      createdAt: new Date("2026-04-13T18:46:22.262Z"),
    },
  ],
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `items`                                                                        | [models.EventListItemResponse](../../models/eventlistitemresponse.md)[]        | :heavy_check_mark:                                                             | Items in this page                                                             |
| `nextCursor`                                                                   | *string*                                                                       | :heavy_check_mark:                                                             | Cursor for the next page, null if last page                                    |
| `releases`                                                                     | [operations.ListEventsRelease](../../models/operations/listeventsrelease.md)[] | :heavy_minus_sign:                                                             | Releases the listed events reference, with ?include=releases                   |