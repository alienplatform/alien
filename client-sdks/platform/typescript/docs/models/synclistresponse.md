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
      retryRequested: false,
      updateState: {
        active: {
          id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
          status: "queued",
          reasons: [],
          targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          changedKeys: [],
          blocker: {
            kind: "other",
            message: "<value>",
            signedSetupUpdate: {
              releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
              templateDigest: "<value>",
            },
          },
          cause: {
            kind: "promotion",
            at: new Date("2026-12-01T10:04:11.187Z"),
            releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          },
          requestedAt: new Date("2025-05-23T02:17:08.731Z"),
        },
        next: {
          id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
          status: "superseded",
          reasons: [
            "redeploy",
          ],
          targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          changedKeys: [
            "<value 1>",
            "<value 2>",
            "<value 3>",
          ],
          blocker: {
            kind: "other",
            message: "<value>",
            signedSetupUpdate: {
              releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
              templateDigest: "<value>",
            },
          },
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
          blocker: {
            kind: "other",
            message: "<value>",
            signedSetupUpdate: {
              releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
              templateDigest: "<value>",
            },
          },
          cause: {
            kind: "promotion",
            at: new Date("2026-12-01T10:04:11.187Z"),
            releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
          },
          requestedAt: new Date("2026-04-09T17:19:50.603Z"),
        },
      },
      createdAt: new Date("2025-12-30T11:04:55.945Z"),
      updatedAt: new Date("2025-06-15T22:31:32.420Z"),
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
      workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
      configurationRevision: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
      userEnvironmentVariables: [
        {
          name: "<value>",
          value: "<value>",
          type: "plain",
          targetResources: [
            "<value 1>",
            "<value 2>",
            "<value 3>",
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