# UpdateDeploymentInputsResponse

## Example Usage

```typescript
import { UpdateDeploymentInputsResponse } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentInputsResponse = {
  inputs: [
    {
      description: "savour duffel dredger",
      id: "<id>",
      kind: "integer",
      label: "<value>",
      providedBy: [
        "developer",
      ],
      required: false,
    },
  ],
  values: {},
  providedInputIds: [
    "<value 1>",
  ],
  runtimeUpdateRequested: false,
  outcome: "accepted",
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

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `inputs`                                                                                           | [models.UpdateDeploymentInputsResponseInput](../models/updatedeploymentinputsresponseinput.md)[]   | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `values`                                                                                           | Record<string, *models.StackInputValueRequest*>                                                    | :heavy_check_mark:                                                                                 | Current non-secret input values. Secret values are never returned.                                 |
| `providedInputIds`                                                                                 | *string*[]                                                                                         | :heavy_check_mark:                                                                                 | Input IDs that currently have a value, including redacted secrets.                                 |
| `runtimeUpdateRequested`                                                                           | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `outcome`                                                                                          | [models.UpdateDeploymentInputsResponseOutcome](../models/updatedeploymentinputsresponseoutcome.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `operation`                                                                                        | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md)           | :heavy_check_mark:                                                                                 | N/A                                                                                                |