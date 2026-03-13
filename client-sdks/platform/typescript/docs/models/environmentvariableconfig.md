# EnvironmentVariableConfig

## Example Usage

```typescript
import { EnvironmentVariableConfig } from "@aliendotdev/platform-api/models";

let value: EnvironmentVariableConfig = {
  name: "<value>",
  value: "<value>",
  type: "plain",
  targetResources: null,
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | Variable name                                                              |
| `value`                                                                    | *string*                                                                   | :heavy_check_mark:                                                         | Variable value (encrypted in database)                                     |
| `type`                                                                     | [models.EnvironmentVariableType](../models/environmentvariabletype.md)     | :heavy_check_mark:                                                         | Variable type (plain or secret)                                            |
| `targetResources`                                                          | *string*[]                                                                 | :heavy_check_mark:                                                         | Target resource patterns (null = all resources, array = wildcard patterns) |