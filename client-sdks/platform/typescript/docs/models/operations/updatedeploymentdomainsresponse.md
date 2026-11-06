# UpdateDeploymentDomainsResponse

Domain settings saved.

## Example Usage

```typescript
import { UpdateDeploymentDomainsResponse } from "@alienplatform/platform-api/models/operations";

let value: UpdateDeploymentDomainsResponse = {
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
    cause: {
      kind: "volume-restore",
      at: new Date("2025-05-23T02:17:08.731Z"),
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    },
    requestedAt: new Date("2025-01-25T18:29:02.081Z"),
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `outcome`                                                                                              | [operations.UpdateDeploymentDomainsOutcome](../../models/operations/updatedeploymentdomainsoutcome.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `operation`                                                                                            | [models.DeploymentUpdateOperationSummary](../../models/deploymentupdateoperationsummary.md)            | :heavy_check_mark:                                                                                     | N/A                                                                                                    |