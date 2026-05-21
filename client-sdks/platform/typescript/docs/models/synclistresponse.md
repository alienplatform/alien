# SyncListResponse

Full deployment records for manager operation

## Example Usage

```typescript
import { SyncListResponse } from "@alienplatform/platform-api/models";

let value: SyncListResponse = {
  deployments: [
    {
      id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      name: "acme-prod",
      status: "deleted",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      platform: "aws",
      deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
      stackSettings: {},
      currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      retryRequested: true,
      createdAt: new Date("2024-11-06T19:21:19.680Z"),
      updatedAt: new Date("2026-10-15T16:14:26.325Z"),
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
      userEnvironmentVariables: [],
    },
  ],
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `deployments`                                                                  | [models.SyncListResponseDeployment](../models/synclistresponsedeployment.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |