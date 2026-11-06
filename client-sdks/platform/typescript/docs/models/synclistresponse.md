# SyncListResponse

Full deployment records for manager operation

## Example Usage

```typescript
import { SyncListResponse } from "@alienplatform/platform-api/models";

let value: SyncListResponse = {
  deployments: [
    {
      id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
      name: "acme-prod",
      status: "retained",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
      platform: "aws",
      deploymentProtocolVersion: 492804,
      deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
      purpose: "encryption",
      stackSettings: {},
      currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      releaseChannel: "<value>",
      protected: false,
      signingFingerprint: "<value>",
      retryRequested: true,
      updateState: {
        active: {
          id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
          status: "blocked",
          reasons: [
            "release",
          ],
          targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          changedKeys: [],
          cause: {
            kind: "volume-restore",
            at: new Date("2025-05-23T02:17:08.731Z"),
            releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          },
          requestedAt: new Date("2025-05-26T07:07:30.861Z"),
        },
        next: {
          id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
          status: "superseded",
          reasons: [
            "release",
          ],
          targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          changedKeys: [
            "<value 1>",
          ],
          cause: null,
          requestedAt: new Date("2025-07-14T02:26:55.487Z"),
        },
        latest: {
          id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
          status: "failed",
          reasons: [],
          targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          changedKeys: [
            "<value 1>",
            "<value 2>",
          ],
          cause: {
            kind: "volume-restore",
            at: new Date("2025-05-23T02:17:08.731Z"),
            releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          },
          requestedAt: new Date("2026-10-10T15:20:06.831Z"),
        },
      },
      createdAt: new Date("2025-05-11T14:21:02.750Z"),
      updatedAt: new Date("2026-07-20T04:30:40.118Z"),
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
      workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
      configurationRevision: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
      userEnvironmentVariables: null,
    },
  ],
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `deployments`                                                                  | [models.SyncListResponseDeployment](../models/synclistresponsedeployment.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |