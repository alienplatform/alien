# DeploymentInfoResponse

## Example Usage

```typescript
import { DeploymentInfoResponse } from "@alienplatform/manager-api/models";

let value: DeploymentInfoResponse = {
  commands: {
    deploymentId: "<id>",
    url: "https://repentant-cake.biz",
  },
  platform: "machines",
  resources: {
    "key": {
      resourceType: "<value>",
    },
  },
  status: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `commands`                                                         | [models.CommandsInfo](../models/commandsinfo.md)                   | :heavy_check_mark:                                                 | N/A                                                                |
| `error`                                                            | *any*                                                              | :heavy_minus_sign:                                                 | N/A                                                                |
| `platform`                                                         | [models.PlatformEnum](../models/platformenum.md)                   | :heavy_check_mark:                                                 | Represents the target cloud platform.                              |
| `resources`                                                        | Record<string, [models.ResourceEntry](../models/resourceentry.md)> | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |