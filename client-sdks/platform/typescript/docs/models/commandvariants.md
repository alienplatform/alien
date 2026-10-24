# CommandVariants

## Example Usage

```typescript
import { CommandVariants } from "@alienplatform/platform-api/models";

let value: CommandVariants = {
  linux: {
    install: "<value>",
    command: "<value>",
  },
  mac: {
    install: "<value>",
    command: "<value>",
  },
  windows: {
    install: "<value>",
    command: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `linux`                                                                                          | [models.Linux](../models/linux.md)                                                               | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `mac`                                                                                            | [models.Mac](../models/mac.md)                                                                   | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `windows`                                                                                        | [models.DeploymentPreparationResponseWindows](../models/deploymentpreparationresponsewindows.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |