# ListManagerEventsResponse

Retrieved events.

## Example Usage

```typescript
import { ListManagerEventsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListManagerEventsResponse = {
  items: [
    {
      id: "event_MtSA24M3pWuAkQYxgZxuRIg6I5i2",
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      debugSessionId: "dbg_HOXmkmT9UPYlsnxqSNlEGoXLGcNV",
      data: {
        type: "DeploymentSetupComputeIgnored",
        deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
        sent: {},
        kept: {},
      },
      state: "started",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
      createdAt: new Date("2024-08-21T19:38:07.197Z"),
      workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
    },
  ],
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `items`                                 | [models.Event](../../models/event.md)[] | :heavy_check_mark:                      | N/A                                     |