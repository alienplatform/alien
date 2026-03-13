# DeploymentListItemResponse

## Example Usage

```typescript
import { DeploymentListItemResponse } from "@aliendotdev/platform-api/models";

let value: DeploymentListItemResponse = {
  id: "ag_pnj2da55wi5sxbdcav9t273je",
  name: "<value>",
  status: "refresh-failed",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  platform: "test",
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  retryRequested: false,
  createdAt: new Date("2025-02-20T18:21:31.698Z"),
  updatedAt: new Date("2026-01-02T04:46:33.633Z"),
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
  release: {
    id: "rel_WbhQgksrawSKIpEN0NAssHX9",
    gitMetadata: {
      commitSha: "dc36199b2234c6586ebe05ec94078a895c707e29",
      commitMessage:
        "add method to measure Interaction to Next Paint (INP) (#36490)",
      commitRef: "main",
      commitDate: new Date("2025-09-29T12:00:00Z"),
      dirty: true,
      remoteUrl: "https://github.com/alienplatform/alien",
      commitAuthorName: "John Doe",
      commitAuthorEmail: "john@example.com",
      commitAuthorLogin: "johndoe",
      commitAuthorAvatarUrl: "https://github.com/johndoe.png",
    },
    createdAt: new Date("2025-01-26T19:54:46.712Z"),
  },
  deploymentGroup: {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    name: "prod-us-east-1",
  },
  project: {
    id: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    name: "my-app",
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | ag_pnj2da55wi5sxbdcav9t273je                                                                  |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `status`                                                                                      | [models.DeploymentListItemResponseStatus](../models/deploymentlistitemresponsestatus.md)      | :heavy_check_mark:                                                                            | Deployment status in the deployment lifecycle                                                 |                                                                                               |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the project.                                                            | prj_mcytp6z3j91f7tn5ryqsfwtr                                                                  |
| `platform`                                                                                    | [models.DeploymentListItemResponsePlatform](../models/deploymentlistitemresponseplatform.md)  | :heavy_check_mark:                                                                            | Target platform for the deployment                                                            |                                                                                               |
| `deploymentGroupId`                                                                           | *string*                                                                                      | :heavy_check_mark:                                                                            | ID of deployment group this deployment belongs to                                             | dg_r27ict8c7vcgsumpj90ackf7b                                                                  |
| `environmentInfo`                                                                             | *models.DeploymentListItemResponseEnvironmentInfoUnion*                                       | :heavy_minus_sign:                                                                            | Cloud environment information                                                                 |                                                                                               |
| `currentReleaseId`                                                                            | *string*                                                                                      | :heavy_minus_sign:                                                                            | ID of the currently deployed release                                                          | rel_WbhQgksrawSKIpEN0NAssHX9                                                                  |
| `desiredReleaseId`                                                                            | *string*                                                                                      | :heavy_minus_sign:                                                                            | ID of the desired release                                                                     | rel_WbhQgksrawSKIpEN0NAssHX9                                                                  |
| `pinnedReleaseId`                                                                             | *string*                                                                                      | :heavy_minus_sign:                                                                            | ID of the pinned release                                                                      | rel_WbhQgksrawSKIpEN0NAssHX9                                                                  |
| `lastHeartbeatAt`                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Timestamp of the last received heartbeat                                                      |                                                                                               |
| `error`                                                                                       | [models.DeploymentListItemResponseError](../models/deploymentlistitemresponseerror.md)        | :heavy_minus_sign:                                                                            | Latest error information if in a failed state                                                 |                                                                                               |
| `retryRequested`                                                                              | *boolean*                                                                                     | :heavy_check_mark:                                                                            | Whether a retry has been requested                                                            |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `managerId`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | ID of the manager                                                                             | mgr_enxscjrqiiu2lrc672hwwuc5                                                                  |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX0                                                                      |
| `release`                                                                                     | [models.DeploymentReleaseInfo](../models/deploymentreleaseinfo.md)                            | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `deploymentGroup`                                                                             | [models.DeploymentGroupInfo](../models/deploymentgroupinfo.md)                                | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `project`                                                                                     | [models.DeploymentProjectInfo](../models/deploymentprojectinfo.md)                            | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |