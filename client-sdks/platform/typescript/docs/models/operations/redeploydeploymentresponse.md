# RedeployDeploymentResponse

Deployment redeployment triggered successfully.

## Example Usage

```typescript
import { RedeployDeploymentResponse } from "@alienplatform/platform-api/models/operations";

let value: RedeployDeploymentResponse = {
  message: "<value>",
  operation: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
    status: "failed",
    reasons: [
      "configuration",
    ],
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
    requestedAt: new Date("2025-01-25T18:29:02.081Z"),
  },
};
```

## Fields

| Field                                                                                       | Type                                                                                        | Required                                                                                    | Description                                                                                 |
| ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `message`                                                                                   | *string*                                                                                    | :heavy_check_mark:                                                                          | N/A                                                                                         |
| `operation`                                                                                 | [models.DeploymentUpdateOperationSummary](../../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                          | N/A                                                                                         |