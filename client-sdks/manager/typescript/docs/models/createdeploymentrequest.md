# CreateDeploymentRequest

## Example Usage

```typescript
import { CreateDeploymentRequest } from "@alienplatform/manager-api/models";

let value: CreateDeploymentRequest = {
  name: "<value>",
  platform: "test",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `deploymentGroupId`                                              | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `environmentVariables`                                           | [models.EnvironmentVariable](../models/environmentvariable.md)[] | :heavy_minus_sign:                                               | N/A                                                              |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `platform`                                                       | [models.Platform](../models/platform.md)                         | :heavy_check_mark:                                               | Represents the target cloud platform.                            |
| `stackSettings`                                                  | [models.StackSettings](../models/stacksettings.md)               | :heavy_minus_sign:                                               | N/A                                                              |