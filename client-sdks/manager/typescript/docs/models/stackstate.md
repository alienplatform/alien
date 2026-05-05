# StackState

Represents the collective state of all resources in a stack, including platform and pending actions.

## Example Usage

```typescript
import { StackState } from "@alienplatform/manager-api/models";

let value: StackState = {
  platform: "gcp",
  resourcePrefix: "<value>",
  resources: {},
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `platform`                                                                   | [models.PlatformEnum](../models/platformenum.md)                             | :heavy_check_mark:                                                           | Represents the target cloud platform.                                        |
| `resourcePrefix`                                                             | *string*                                                                     | :heavy_check_mark:                                                           | A prefix used for resource naming to ensure uniqueness across deployments.   |
| `resources`                                                                  | Record<string, [models.StackResourceState](../models/stackresourcestate.md)> | :heavy_check_mark:                                                           | The state of individual resources, keyed by resource ID.                     |