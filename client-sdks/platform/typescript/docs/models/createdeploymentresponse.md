# CreateDeploymentResponse

## Example Usage

```typescript
import { CreateDeploymentResponse } from "@alienplatform/platform-api/models";

let value: CreateDeploymentResponse = {
  deployment: {
    id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
    name: "acme-prod",
    status: "refresh-failed",
    projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
    platform: "kubernetes",
    deploymentProtocolVersion: 883690,
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
    createdAt: new Date("2026-04-09T17:19:50.603Z"),
    updatedAt: new Date("2025-03-12T21:42:58.399Z"),
    managerId: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
    workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
    installId: "<id>",
  },
  deploymentModel: "push",
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
| `airgapped`                                                                                            | [models.Airgapped](../models/airgapped.md)                                                             | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |