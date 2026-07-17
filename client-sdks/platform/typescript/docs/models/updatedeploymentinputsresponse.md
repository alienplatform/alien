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
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `inputs`                                                                                         | [models.UpdateDeploymentInputsResponseInput](../models/updatedeploymentinputsresponseinput.md)[] | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `values`                                                                                         | Record<string, *models.StackInputValueRequest*>                                                  | :heavy_check_mark:                                                                               | Current non-secret input values. Secret values are never returned.                               |
| `providedInputIds`                                                                               | *string*[]                                                                                       | :heavy_check_mark:                                                                               | Input IDs that currently have a value, including redacted secrets.                               |
| `runtimeUpdateRequested`                                                                         | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |