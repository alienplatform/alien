# CommandListItemResponse

## Example Usage

```typescript
import { CommandListItemResponse } from "@alienplatform/platform-api/models";

let value: CommandListItemResponse = {
  id: "cmd_2sxjXxvOYct7IohT3ukliAzf",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
  name: "<value>",
  state: "EXPIRED",
  deploymentModel: "push",
  target: {
    resourceId: "<id>",
    resourceType: "container",
  },
  attempt: 4724.38,
  deadline: new Date("2024-09-28T00:43:03.594Z"),
  requestSizeBytes: 9605.97,
  responseSizeBytes: 7449.44,
  createdAt: new Date("2026-08-24T15:49:02.935Z"),
  dispatchedAt: new Date("2025-09-01T19:33:50.383Z"),
  completedAt: null,
  error: {
    "key": "<value>",
  },
  deployment: {
    id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    name: "<value>",
    managerId: "<id>",
  },
  project: {
    id: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          | Example                                                                                              |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `id`                                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the command.                                                                   | cmd_2sxjXxvOYct7IohT3ukliAzf                                                                         |
| `deploymentId`                                                                                       | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the deployment.                                                                | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                         |
| `projectId`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the project.                                                                   | prj_mcytp6z3j91f7tn5ryqsfwtr                                                                         |
| `workspaceId`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the workspace.                                                                 | ws_It13CUaGEhLLAB87simX0                                                                             |
| `name`                                                                                               | *string*                                                                                             | :heavy_check_mark:                                                                                   | Command name (e.g., 'analyze-repository', 'sync-data')                                               |                                                                                                      |
| `state`                                                                                              | [models.CommandListItemResponseState](../models/commandlistitemresponsestate.md)                     | :heavy_check_mark:                                                                                   | Command states in the Commands protocol lifecycle                                                    |                                                                                                      |
| `deploymentModel`                                                                                    | [models.CommandListItemResponseDeploymentModel](../models/commandlistitemresponsedeploymentmodel.md) | :heavy_check_mark:                                                                                   | Delivery mode for this command (push/pull), derived from the target at creation time                 |                                                                                                      |
| `target`                                                                                             | [models.CommandListItemResponseTarget](../models/commandlistitemresponsetarget.md)                   | :heavy_check_mark:                                                                                   | Resource the command is addressed to; null on commands created before target routing                 |                                                                                                      |
| `attempt`                                                                                            | *number*                                                                                             | :heavy_check_mark:                                                                                   | Current attempt number                                                                               |                                                                                                      |
| `deadline`                                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | Optional deadline for command execution                                                              |                                                                                                      |
| `requestSizeBytes`                                                                                   | *number*                                                                                             | :heavy_check_mark:                                                                                   | Size of command params in bytes                                                                      |                                                                                                      |
| `responseSizeBytes`                                                                                  | *number*                                                                                             | :heavy_check_mark:                                                                                   | Size of command response in bytes                                                                    |                                                                                                      |
| `createdAt`                                                                                          | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | When the command was created                                                                         |                                                                                                      |
| `dispatchedAt`                                                                                       | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | When the command was dispatched to the deployment                                                    |                                                                                                      |
| `completedAt`                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | When the command completed                                                                           |                                                                                                      |
| `error`                                                                                              | Record<string, *any*>                                                                                | :heavy_check_mark:                                                                                   | Error details if command failed                                                                      |                                                                                                      |
| `result`                                                                                             | *any*                                                                                                | :heavy_minus_sign:                                                                                   | Decoded command result when available                                                                |                                                                                                      |
| `deployment`                                                                                         | [models.CommandDeploymentInfo](../models/commanddeploymentinfo.md)                                   | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `project`                                                                                            | [models.CommandProjectInfo](../models/commandprojectinfo.md)                                         | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |                                                                                                      |