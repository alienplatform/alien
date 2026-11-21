# DeploymentInputsResponse

## Example Usage

```typescript
import { DeploymentInputsResponse } from "@alienplatform/platform-api/models";

let value: DeploymentInputsResponse = {
  inputs: [
    {
      description: "although towards councilman lest",
      id: "<id>",
      kind: "integer",
      label: "<value>",
      providedBy: [
        "deployer",
      ],
      required: true,
    },
  ],
  values: {},
  providedInputIds: [
    "<value 1>",
  ],
};
```

## Fields

| Field                                                                                                                                                              | Type                                                                                                                                                               | Required                                                                                                                                                           | Description                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `inputs`                                                                                                                                                           | [models.DeploymentInputsResponseInput](../models/deploymentinputsresponseinput.md)[]                                                                               | :heavy_check_mark:                                                                                                                                                 | N/A                                                                                                                                                                |
| `values`                                                                                                                                                           | Record<string, *models.StackInputValueRequest*>                                                                                                                    | :heavy_check_mark:                                                                                                                                                 | Current non-secret input values. Secret values are never returned.                                                                                                 |
| `providedInputIds`                                                                                                                                                 | *string*[]                                                                                                                                                         | :heavy_check_mark:                                                                                                                                                 | Input IDs that currently have a value, including redacted secrets.                                                                                                 |
| `held`                                                                                                                                                             | Record<string, [models.DeploymentInputsResponseHeld](../models/deploymentinputsresponseheld.md)>                                                                   | :heavy_minus_sign:                                                                                                                                                 | Answers the manager stores, by input id: when and who. `values` holds an empty string for them; the value is read from the manager with the value-access endpoint. |
| `heldInputIds`                                                                                                                                                     | *string*[]                                                                                                                                                         | :heavy_minus_sign:                                                                                                                                                 | Every visible input the manager stores, answered or not. Send these as `inputValueRefs`, never as `inputValues`.                                                   |