# GetDeploymentEnvironmentVariablesResponse

Environment override metadata.

## Example Usage

```typescript
import { GetDeploymentEnvironmentVariablesResponse } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentEnvironmentVariablesResponse = {
  variables: [
    {
      name: "<value>",
      type: "secret",
      targetResources: [
        "<value 1>",
        "<value 2>",
      ],
    },
  ],
  storedInManager: true,
  slots: [],
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `variables`                                                                                                                                                | [operations.Variable](../../models/operations/variable.md)[]                                                                                               | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `storedInManager`                                                                                                                                          | *boolean*                                                                                                                                                  | :heavy_check_mark:                                                                                                                                         | Whether the deployment's manager stores values. When true, send new values as `valueRef` after writing them to the manager with the value-access endpoint. |
| `slots`                                                                                                                                                    | [models.ValueSlotState](../../models/valueslotstate.md)[]                                                                                                  | :heavy_check_mark:                                                                                                                                         | One entry per variable: whether it is set, and who stored it and when.                                                                                     |