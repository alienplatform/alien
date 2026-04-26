# Command

## Example Usage

```typescript
import { Command } from "@alienplatform/platform-api/models";

let value: Command = {
  id: "cmd_2sxjXxvOYct7IohT3ukliAzf",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
  name: "<value>",
  state: "SUCCEEDED",
  deploymentModel: "push",
  attempt: 9009.54,
  deadline: new Date("2025-08-12T01:46:10.944Z"),
  requestSizeBytes: 7714.06,
  responseSizeBytes: 9249.44,
  createdAt: new Date("2025-06-13T18:52:36.177Z"),
  dispatchedAt: new Date("2025-08-18T01:19:40.077Z"),
  completedAt: new Date("2024-12-27T01:09:12.395Z"),
  error: {
    "key": "<value>",
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the command.                                                            | cmd_2sxjXxvOYct7IohT3ukliAzf                                                                  |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the project.                                                            | prj_mcytp6z3j91f7tn5ryqsfwtr                                                                  |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX0                                                                      |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Command name (e.g., 'analyze-repository', 'sync-data')                                        |                                                                                               |
| `state`                                                                                       | [models.CommandState](../models/commandstate.md)                                              | :heavy_check_mark:                                                                            | Command states in the Commands protocol lifecycle                                             |                                                                                               |
| `deploymentModel`                                                                             | [models.CommandDeploymentModel](../models/commanddeploymentmodel.md)                          | :heavy_check_mark:                                                                            | Deployment model captured from deployment at creation time                                    |                                                                                               |
| `attempt`                                                                                     | *number*                                                                                      | :heavy_check_mark:                                                                            | Current attempt number                                                                        |                                                                                               |
| `deadline`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | Optional deadline for command execution                                                       |                                                                                               |
| `requestSizeBytes`                                                                            | *number*                                                                                      | :heavy_check_mark:                                                                            | Size of command params in bytes                                                               |                                                                                               |
| `responseSizeBytes`                                                                           | *number*                                                                                      | :heavy_check_mark:                                                                            | Size of command response in bytes                                                             |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When the command was created                                                                  |                                                                                               |
| `dispatchedAt`                                                                                | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When the command was dispatched to the deployment                                             |                                                                                               |
| `completedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When the command completed                                                                    |                                                                                               |
| `error`                                                                                       | Record<string, *any*>                                                                         | :heavy_check_mark:                                                                            | Error details if command failed                                                               |                                                                                               |