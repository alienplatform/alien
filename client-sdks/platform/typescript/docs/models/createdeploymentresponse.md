# CreateDeploymentResponse

## Example Usage

```typescript
import { CreateDeploymentResponse } from "@alienplatform/platform-api/models";

let value: CreateDeploymentResponse = {
  deployment: {
    id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    name: "acme-prod",
    status: "running",
    projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    platform: "kubernetes",
    deploymentProtocolVersion: 883690,
    deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
    purpose: "encryption",
    stackSettings: {},
    currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    releaseChannel: "<value>",
    retryRequested: false,
    updateState: {
      active: {
        id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
        status: "queued",
        reasons: [],
        targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        changedKeys: [],
        cause: {
          kind: "volume-restore",
          at: new Date("2024-04-20T16:58:36.345Z"),
          releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        },
        requestedAt: new Date("2024-05-03T02:14:14.983Z"),
      },
      next: {
        id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
        status: "applying",
        reasons: [],
        targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        changedKeys: [
          "<value 1>",
          "<value 2>",
          "<value 3>",
        ],
        cause: {
          kind: "volume-restore",
          at: new Date("2024-04-20T16:58:36.345Z"),
          releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        },
        requestedAt: new Date("2025-07-21T11:59:25.062Z"),
      },
      latest: {
        id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
        status: "queued",
        reasons: [],
        targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        changedKeys: [
          "<value 1>",
          "<value 2>",
        ],
        cause: {
          kind: "volume-restore",
          at: new Date("2024-04-20T16:58:36.345Z"),
          releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        },
        requestedAt: new Date("2026-06-26T04:52:21.740Z"),
      },
    },
    createdAt: new Date("2024-01-13T13:20:22.491Z"),
    updatedAt: new Date("2025-07-24T15:47:13.039Z"),
    managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
  },
  deploymentModel: "pull",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `deployment`                                                                                           | [models.Deployment](../models/deployment.md)                                                           | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `deploymentModel`                                                                                      | [models.CreateDeploymentResponseDeploymentModel](../models/createdeploymentresponsedeploymentmodel.md) | :heavy_check_mark:                                                                                     | Effective deployment model persisted for the deployment.                                               |
| `installation`                                                                                         | [models.DeploymentInstallation](../models/deploymentinstallation.md)                                   | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `managementExternalId`                                                                                 | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `runtimeEncryptionKey`                                                                                 | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `token`                                                                                                | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Deployment token (returned for deployment-group creation and await-registration reservations)          |