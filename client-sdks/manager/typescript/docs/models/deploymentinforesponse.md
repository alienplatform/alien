# DeploymentInfoResponse

## Example Usage

```typescript
import { DeploymentInfoResponse } from "@alienplatform/manager-api/models";

let value: DeploymentInfoResponse = {
  commands: {
    deploymentId: "<id>",
    url: "https://repentant-cake.biz",
  },
  platform: "local",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "function",
      },
      dependencies: [],
      lifecycle: "live-on-setup",
    },
  },
  status: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `commands`                                                         | [models.CommandsInfo](../models/commandsinfo.md)                   | :heavy_check_mark:                                                 | N/A                                                                |
| `platform`                                                         | [models.PlatformEnum](../models/platformenum.md)                   | :heavy_check_mark:                                                 | Represents the target cloud platform.                              |
| `resources`                                                        | Record<string, [models.ResourceEntry](../models/resourceentry.md)> | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |