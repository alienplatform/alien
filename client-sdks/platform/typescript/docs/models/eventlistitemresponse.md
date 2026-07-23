# EventListItemResponse

## Example Usage

```typescript
import { EventListItemResponse } from "@alienplatform/platform-api/models";

let value: EventListItemResponse = {
  id: "event_MtSA24M3pWuAkQYxgZxuRI",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  debugSessionId: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
  data: {
    type: "GeneratingCloudFormationTemplate",
  },
  state: "success",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  createdAt: new Date("2024-08-03T01:48:11.595Z"),
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
| `data`                                                                                        | *models.EventListItemResponseDataUnion*                                                       | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `state`                                                                                       | *models.EventListItemResponseStateUnion*                                                      | :heavy_check_mark:                                                                            | Represents the state of an event                                                              |                                                                                               |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the project.                                                            | prj_mcytp6z3j91f7tn5ryqsfwtr                                                                  |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX0                                                                      |
| `releaseCreatedAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | createdAt of the event's referenced release, included when ?include=releaseCreatedAt is used  |                                                                                               |
