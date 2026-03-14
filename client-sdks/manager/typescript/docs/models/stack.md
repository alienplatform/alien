# Stack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { Stack } from "@alienplatform/manager-api/models";

let value: Stack = {
  id: "<id>",
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
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `id`                                                                          | *string*                                                                      | :heavy_check_mark:                                                            | Unique identifier for the stack                                               |
| `permissions`                                                                 | [models.PermissionsConfig](../models/permissionsconfig.md)                    | :heavy_minus_sign:                                                            | Combined permissions configuration that contains both profiles and management |
| `resources`                                                                   | Record<string, [models.ResourceEntry](../models/resourceentry.md)>            | :heavy_check_mark:                                                            | Map of resource IDs to their configurations and lifecycle settings            |