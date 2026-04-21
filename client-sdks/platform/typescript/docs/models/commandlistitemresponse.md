# CommandListItemResponse

## Example Usage

```typescript
import { CommandListItemResponse } from "@alienplatform/platform-api/models";

let value: CommandListItemResponse = {
  id: "cmd_2sxjXxvOYct7IohT3ukliAzf",
  deploymentId: "ag_pnj2da55wi5sxbdcav9t273je",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
  name: "<value>",
  state: "EXPIRED",
  deploymentModel: "push",
  attempt: 6289.82,
  deadline: new Date("2025-06-01T19:00:05.525Z"),
  requestSizeBytes: 2472.9,
  responseSizeBytes: 9605.97,
  createdAt: new Date("2024-06-09T12:02:52.168Z"),
  dispatchedAt: new Date("2026-08-24T15:49:02.935Z"),
  completedAt: new Date("2025-09-01T19:33:50.383Z"),
  error: null,
  deployment: {
    id: "ag_pnj2da55wi5sxbdcav9t273je",
    name: "<value>",
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
| `deploymentId`                                                                                       | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the deployment.                                                                | ag_pnj2da55wi5sxbdcav9t273je                                                                         |
| `projectId`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the project.                                                                   | prj_mcytp6z3j91f7tn5ryqsfwtr                                                                         |
| `workspaceId`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the workspace.                                                                 | ws_It13CUaGEhLLAB87simX0                                                                             |
| `name`                                                                                               | *string*                                                                                             | :heavy_check_mark:                                                                                   | Command name (e.g., 'analyze-repository', 'sync-data')                                               |                                                                                                      |
| `state`                                                                                              | [models.CommandListItemResponseState](../models/commandlistitemresponsestate.md)                     | :heavy_check_mark:                                                                                   | Command states in the Commands protocol lifecycle                                                    |                                                                                                      |
| `deploymentModel`                                                                                    | [models.CommandListItemResponseDeploymentModel](../models/commandlistitemresponsedeploymentmodel.md) | :heavy_check_mark:                                                                                   | Deployment model captured from deployment at creation time                                           |                                                                                                      |
| `attempt`                                                                                            | *number*                                                                                             | :heavy_check_mark:                                                                                   | Current attempt number                                                                               |                                                                                                      |
| `deadline`                                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | Optional deadline for command execution                                                              |                                                                                                      |
| `requestSizeBytes`                                                                                   | *number*                                                                                             | :heavy_check_mark:                                                                                   | Size of command params in bytes                                                                      |                                                                                                      |
| `responseSizeBytes`                                                                                  | *number*                                                                                             | :heavy_check_mark:                                                                                   | Size of command response in bytes                                                                    |                                                                                                      |
| `createdAt`                                                                                          | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | When the command was created                                                                         |                                                                                                      |
| `dispatchedAt`                                                                                       | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | When the command was dispatched to the deployment                                                    |                                                                                                      |
| `completedAt`                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | When the command completed                                                                           |                                                                                                      |
| `error`                                                                                              | Record<string, *any*>                                                                                | :heavy_check_mark:                                                                                   | Error details if command failed                                                                      |                                                                                                      |
| `deployment`                                                                                         | [models.CommandDeploymentInfo](../models/commanddeploymentinfo.md)                                   | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `project`                                                                                            | [models.CommandProjectInfo](../models/commandprojectinfo.md)                                         | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |                                                                                                      |