# Event

## Example Usage

```typescript
import { Event } from "@alienplatform/platform-api/models";

let value: Event = {
  id: "event_MtSA24M3pWuAkQYxgZxuRIg6I5i2",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
  debugSessionId: "dbg_HOXmkmT9UPYlsnxqSNlEGoXLGcNV",
  data: {
    type: "CloudAccountComputeCleanupRequested",
    cloudAccountId: "<id>",
    actor: {
      kind: "user",
      id: "<id>",
    },
  },
  state: "started",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  createdAt: new Date("2025-01-03T01:01:38.642Z"),
  workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the event.                                                              | event_MtSA24M3pWuAkQYxgZxuRIg6I5i2                                                            |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                                              |
| `releaseId`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | Unique identifier for the release.                                                            | rel_WbhQgksrawSKIpEN0NAssHX9E2Su                                                              |
| `debugSessionId`                                                                              | *string*                                                                                      | :heavy_minus_sign:                                                                            | Unique identifier for the debug session.                                                      | dbg_HOXmkmT9UPYlsnxqSNlEGoXLGcNV                                                              |
| `data`                                                                                        | *models.EventDataUnion*                                                                       | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `state`                                                                                       | *models.EventStateUnion*                                                                      | :heavy_check_mark:                                                                            | Represents the state of an event                                                              |                                                                                               |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the project.                                                            | prj_mcytp6z3j91f7tn5ryqsfwtrdqkv                                                              |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX07Xb                                                                   |