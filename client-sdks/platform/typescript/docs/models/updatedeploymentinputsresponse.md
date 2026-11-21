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

| Field                                                                                                                                                              | Type                                                                                                                                                               | Required                                                                                                                                                           | Description                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `inputs`                                                                                                                                                           | [models.UpdateDeploymentInputsResponseInput](../models/updatedeploymentinputsresponseinput.md)[]                                                                   | :heavy_check_mark:                                                                                                                                                 | N/A                                                                                                                                                                |
| `values`                                                                                                                                                           | Record<string, *models.StackInputValueRequest*>                                                                                                                    | :heavy_check_mark:                                                                                                                                                 | Current non-secret input values. Secret values are never returned.                                                                                                 |
| `providedInputIds`                                                                                                                                                 | *string*[]                                                                                                                                                         | :heavy_check_mark:                                                                                                                                                 | Input IDs that currently have a value, including redacted secrets.                                                                                                 |
| `held`                                                                                                                                                             | Record<string, [models.UpdateDeploymentInputsResponseHeld](../models/updatedeploymentinputsresponseheld.md)>                                                       | :heavy_minus_sign:                                                                                                                                                 | Answers the manager stores, by input id: when and who. `values` holds an empty string for them; the value is read from the manager with the value-access endpoint. |
| `heldInputIds`                                                                                                                                                     | *string*[]                                                                                                                                                         | :heavy_minus_sign:                                                                                                                                                 | Every visible input the manager stores, answered or not. Send these as `inputValueRefs`, never as `inputValues`.                                                   |
| `runtimeUpdateRequested`                                                                                                                                           | *boolean*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                 | N/A                                                                                                                                                                |
| `outcome`                                                                                                                                                          | [models.UpdateDeploymentInputsResponseOutcome](../models/updatedeploymentinputsresponseoutcome.md)                                                                 | :heavy_check_mark:                                                                                                                                                 | N/A                                                                                                                                                                |
| `operation`                                                                                                                                                        | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md)                                                                           | :heavy_check_mark:                                                                                                                                                 | N/A                                                                                                                                                                |