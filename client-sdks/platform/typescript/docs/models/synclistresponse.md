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
      status: "teardown-failed",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      platform: "aws",
      deploymentProtocolVersion: 492804,
      deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
      purpose: "encryption",
      stackSettings: {},
      currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      releaseChannel: "<value>",
      retryRequested: false,
      createdAt: new Date("2025-12-30T11:04:55.945Z"),
      updatedAt: new Date("2025-06-15T22:31:32.420Z"),
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
      userEnvironmentVariables: [
        {
          name: "<value>",
          value: "<value>",
          type: "plain",
          targetResources: [
            "<value 1>",
          ],
        },
      ],
    },
  ],
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `deployments`                                                                  | [models.SyncListResponseDeployment](../models/synclistresponsedeployment.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |