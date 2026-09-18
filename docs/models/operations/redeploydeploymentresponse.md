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
    requestedAt: new Date("2025-01-15T20:32:07.502Z"),
  },
};
```

## Fields

| Field                                                                                       | Type                                                                                        | Required                                                                                    | Description                                                                                 |
| ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `message`                                                                                   | *string*                                                                                    | :heavy_check_mark:                                                                          | N/A                                                                                         |
| `operation`                                                                                 | [models.DeploymentUpdateOperationSummary](../../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                          | N/A                                                                                         |