# CreateDeploymentResponseBody

Existing deployment returned for idempotent deployment-group registration.

## Example Usage

```typescript
import { CreateDeploymentResponseBody } from "@alienplatform/platform-api/models/operations";

let value: CreateDeploymentResponseBody = {
  deployment: {
    id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    name: "acme-prod",
    status: "deleting",
    projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    platform: "aws",
    deploymentProtocolVersion: 108843,
    deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
    stackSettings: {},
    currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    retryRequested: true,
    createdAt: new Date("2024-05-26T23:54:19.383Z"),
    updatedAt: new Date("2026-10-15T18:20:54.194Z"),
    managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `deployment`                                                       | [models.Deployment](../../models/deployment.md)                    | :heavy_check_mark:                                                 | N/A                                                                |
| `token`                                                            | *string*                                                           | :heavy_minus_sign:                                                 | Deployment token (only returned when using deployment group token) |