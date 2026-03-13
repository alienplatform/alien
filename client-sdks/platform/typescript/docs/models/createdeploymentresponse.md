# CreateDeploymentResponse

## Example Usage

```typescript
import { CreateDeploymentResponse } from "@aliendotdev/platform-api/models";

let value: CreateDeploymentResponse = {
  deployment: {
    id: "ag_pnj2da55wi5sxbdcav9t273je",
    name: "acme-prod",
    status: "running",
    projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    platform: "kubernetes",
    deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
    stackSettings: {},
    currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    retryRequested: false,
    createdAt: new Date("2024-11-27T20:34:51.965Z"),
    updatedAt: new Date("2025-11-27T13:33:53.835Z"),
    managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
  },
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `deployment`                                                  | [models.Deployment](../models/deployment.md)                  | :heavy_check_mark:                                            | N/A                                                           |
| `token`                                                       | *string*                                                      | :heavy_minus_sign:                                            | Agent token (only returned when using deployment group token) |