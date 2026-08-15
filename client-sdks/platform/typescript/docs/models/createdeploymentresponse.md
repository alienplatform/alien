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
        status: "applying",
        reasons: [],
        targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        changedKeys: [],
        requestedAt: new Date("2026-04-28T10:12:17.277Z"),
      },
      next: {
        id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
        status: "queued",
        reasons: [],
        targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        changedKeys: [
          "<value 1>",
          "<value 2>",
          "<value 3>",
        ],
        requestedAt: new Date("2025-05-23T02:17:08.731Z"),
      },
      latest: {
        id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
        status: "superseded",
        reasons: [
          "redeploy",
        ],
        targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        changedKeys: [
          "<value 1>",
          "<value 2>",
          "<value 3>",
        ],
        requestedAt: new Date("2024-11-23T19:43:18.153Z"),
      },
    },
    createdAt: new Date("2024-01-02T08:31:30.348Z"),
    updatedAt: new Date("2025-07-14T02:26:55.487Z"),
    managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
  },
  deploymentModel: "push",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `deployment`                                                                                           | [models.Deployment](../models/deployment.md)                                                           | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `deploymentModel`                                                                                      | [models.CreateDeploymentResponseDeploymentModel](../models/createdeploymentresponsedeploymentmodel.md) | :heavy_check_mark:                                                                                     | Effective deployment model persisted for the deployment.                                               |
| `token`                                                                                                | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Deployment token (only returned when using deployment group token)                                     |