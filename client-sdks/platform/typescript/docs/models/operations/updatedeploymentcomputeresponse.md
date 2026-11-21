# UpdateDeploymentComputeResponse

Compute settings saved or already current.

## Example Usage

```typescript
import { UpdateDeploymentComputeResponse } from "@alienplatform/platform-api/models/operations";

let value: UpdateDeploymentComputeResponse = {
  outcome: "accepted",
  operation: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
    status: "failed",
    reasons: [
      "configuration",
    ],
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
    requestedAt: new Date("2025-04-09T03:01:22.653Z"),
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `outcome`                                                                                              | [operations.UpdateDeploymentComputeOutcome](../../models/operations/updatedeploymentcomputeoutcome.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `operation`                                                                                            | [models.DeploymentUpdateOperationSummary](../../models/deploymentupdateoperationsummary.md)            | :heavy_check_mark:                                                                                     | N/A                                                                                                    |