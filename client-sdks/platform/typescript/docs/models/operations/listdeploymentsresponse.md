# ListDeploymentsResponse

Paginated response

## Example Usage

```typescript
import { ListDeploymentsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListDeploymentsResponse = {
  items: [
    {
      id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      name: "<value>",
      status: "running",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      platform: "test",
      deploymentProtocolVersion: 120212,
      deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
      purpose: "storage",
      currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      releaseChannel: "<value>",
      retryRequested: false,
      updateBlocked: false,
      createdAt: new Date("2026-05-14T06:52:18.790Z"),
      updatedAt: new Date("2025-01-26T19:54:46.712Z"),
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
      release: {
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
        createdAt: new Date("2025-09-02T20:50:48.960Z"),
      },
      deploymentGroup: {
        id: "dg_r27ict8c7vcgsumpj90ackf7b",
        name: "prod-us-east-1",
        externalId: "ext_example_01",
      },
      project: {
        id: "prj_mcytp6z3j91f7tn5ryqsfwtr",
        name: "my-app",
      },
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `items`                                                                           | [models.DeploymentListItemResponse](../../models/deploymentlistitemresponse.md)[] | :heavy_check_mark:                                                                | Items in this page                                                                |
| `nextCursor`                                                                      | *string*                                                                          | :heavy_check_mark:                                                                | Cursor for the next page, null if last page                                       |