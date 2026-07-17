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

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `inputs`                                                                             | [models.DeploymentInputsResponseInput](../models/deploymentinputsresponseinput.md)[] | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `values`                                                                             | Record<string, *models.StackInputValueRequest*>                                      | :heavy_check_mark:                                                                   | Current non-secret input values. Secret values are never returned.                   |
| `providedInputIds`                                                                   | *string*[]                                                                           | :heavy_check_mark:                                                                   | Input IDs that currently have a value, including redacted secrets.                   |