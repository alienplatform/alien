# DeploymentInfoSetupConfigEnvironmentVariable

## Example Usage

```typescript
import { DeploymentInfoSetupConfigEnvironmentVariable } from "@alienplatform/platform-api/models";

let value: DeploymentInfoSetupConfigEnvironmentVariable = {
  name: "<value>",
  type: "plain",
  targetResources: [
    "<value 1>",
  ],
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | Variable name                                                              |
| `type`                                                                     | [models.EnvironmentVariableType](../models/environmentvariabletype.md)     | :heavy_check_mark:                                                         | Variable type (plain or secret)                                            |
| `targetResources`                                                          | *string*[]                                                                 | :heavy_check_mark:                                                         | Target resource patterns (null = all resources, array = wildcard patterns) |