# ListDeploymentsResponse

Paginated response

## Example Usage

```typescript
import { ListDeploymentsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListDeploymentsResponse = {
  items: [
    {
      id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
      name: "<value>",
      status: "running",
      lastAirgappedReport: {
        receivedAt: new Date("2026-07-21T03:53:54.471Z"),
        status: "running",
        installedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
        attemptedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
        replicas: {
          ready: 900503,
          desired: 914932,
        },
      },
      lastAirgappedDownload: {
        releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
        downloadedAt: new Date("2024-04-17T05:47:54.417Z"),
      },
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
      platform: "kubernetes",
      deploymentProtocolVersion: 810826,
      deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
      purpose: "sandbox",
      currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      releaseChannel: "<value>",
      retryRequested: false,
      updateBlocked: true,
      createdAt: new Date("2024-06-27T06:57:01.451Z"),
      updatedAt: new Date("2026-11-13T11:54:01.087Z"),
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
      workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
      release: {
        id: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
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
        createdAt: new Date("2025-02-28T16:11:01.301Z"),
      },
      desiredRelease: {
        id: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
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
        createdAt: new Date("2025-02-22T23:32:21.165Z"),
      },
      deploymentGroup: {
        id: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
        name: "prod-us-east-1",
        externalId: "ext_example_01",
      },
      project: {
        id: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
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