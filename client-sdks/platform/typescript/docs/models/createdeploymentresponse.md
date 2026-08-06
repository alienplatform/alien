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
    createdAt: new Date("2024-08-29T09:22:49.431Z"),
    updatedAt: new Date("2024-12-06T20:41:40.649Z"),
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