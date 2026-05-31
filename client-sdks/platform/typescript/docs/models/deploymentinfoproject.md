# DeploymentInfoProject

## Example Usage

```typescript
import { DeploymentInfoProject } from "@alienplatform/platform-api/models";

let value: DeploymentInfoProject = {
  name: "<value>",
  portal: {
    appearance: {},
  },
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `name`                                           | *string*                                         | :heavy_check_mark:                               | N/A                                              |
| `portal`                                         | [models.Portal](../models/portal.md)             | :heavy_check_mark:                               | N/A                                              |
| `stackSummary`                                   | [models.StackSummary](../models/stacksummary.md) | :heavy_minus_sign:                               | N/A                                              |