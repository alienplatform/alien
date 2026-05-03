# Event

## Example Usage

```typescript
import { Event } from "@alienplatform/platform-api/models";

let value: Event = {
  id: "event_MtSA24M3pWuAkQYxgZxuRI",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  debugSessionId: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
  data: {
    stack: "<value>",
    type: "BuildingStack",
  },
  state: "started",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  createdAt: new Date("2025-01-03T01:01:38.642Z"),
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the event.                                                              | event_MtSA24M3pWuAkQYxgZxuRI                                                                  |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `releaseId`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | Unique identifier for the release.                                                            | rel_WbhQgksrawSKIpEN0NAssHX9                                                                  |
| `debugSessionId`                                                                              | *string*                                                                                      | :heavy_minus_sign:                                                                            | Unique identifier for the debug session.                                                      | dbg_HOXmkmT9UPYlsnxqSNlEGoXL                                                                  |
| `data`                                                                                        | *models.Data*                                                                                 | :heavy_check_mark:                                                                            | Represents all possible events in the Alien system                                            |                                                                                               |
| `state`                                                                                       | *models.State*                                                                                | :heavy_check_mark:                                                                            | Represents the state of an event                                                              |                                                                                               |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the project.                                                            | prj_mcytp6z3j91f7tn5ryqsfwtr                                                                  |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX0                                                                      |