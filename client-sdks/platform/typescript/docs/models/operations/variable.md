# Variable

## Example Usage

```typescript
import { Variable } from "@alienplatform/platform-api/models/operations";

let value: Variable = {
  name: "<value>",
  type: "plain",
  targetResources: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | Variable name                                                              |
| `type`                                                                     | [models.EnvironmentVariableType](../../models/environmentvariabletype.md)  | :heavy_check_mark:                                                         | Variable type (plain or secret)                                            |
| `targetResources`                                                          | *string*[]                                                                 | :heavy_check_mark:                                                         | Target resource patterns (null = all resources, array = wildcard patterns) |