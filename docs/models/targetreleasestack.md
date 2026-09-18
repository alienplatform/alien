# TargetReleaseStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { TargetReleaseStack } from "@alienplatform/platform-api/models";

let value: TargetReleaseStack = {
  id: "<id>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      dependencies: [
        {
          id: "<id>",
          type: "<value>",
        },
      ],
      lifecycle: "live",
    },
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `id`                                                                                   | *string*                                                                               | :heavy_check_mark:                                                                     | Unique identifier for the stack                                                        |
| `inputs`                                                                               | [models.TargetReleaseInput](../models/targetreleaseinput.md)[]                         | :heavy_minus_sign:                                                                     | Input definitions required before setup or deployment can proceed.                     |
| `permissions`                                                                          | [models.TargetReleasePermissions](../models/targetreleasepermissions.md)               | :heavy_minus_sign:                                                                     | Combined permissions configuration that contains both profiles and management          |
| `resources`                                                                            | Record<string, [models.TargetReleaseResources](../models/targetreleaseresources.md)>   | :heavy_check_mark:                                                                     | Map of resource IDs to their configurations and lifecycle settings                     |
| `supportedPlatforms`                                                                   | [models.TargetReleaseSupportedPlatform](../models/targetreleasesupportedplatform.md)[] | :heavy_minus_sign:                                                                     | Which platforms this stack supports. When None, all platforms are supported.           |