# ReleaseInfoStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { ReleaseInfoStack } from "@aliendotdev/platform-api/models";

let value: ReleaseInfoStack = {
  id: "<id>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      dependencies: [],
      lifecycle: "frozen",
    },
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `id`                                                                             | *string*                                                                         | :heavy_check_mark:                                                               | Unique identifier for the stack                                                  |
| `permissions`                                                                    | [models.ReleaseInfoPermissions](../models/releaseinfopermissions.md)             | :heavy_minus_sign:                                                               | Combined permissions configuration that contains both profiles and management    |
| `resources`                                                                      | Record<string, [models.ReleaseInfoResources](../models/releaseinforesources.md)> | :heavy_check_mark:                                                               | Map of resource IDs to their configurations and lifecycle settings               |