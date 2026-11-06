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
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `variables`                                                  | [operations.Variable](../../models/operations/variable.md)[] | :heavy_check_mark:                                           | N/A                                                          |