# SyncAcquireResponseStackState

Represents the collective state of all resources in a stack, including platform and pending actions.

## Example Usage

```typescript
import { SyncAcquireResponseStackState } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseStackState = {
  platform: "local",
  resourcePrefix: "<value>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      status: "updating",
      type: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                           | [models.SyncAcquireResponseStackStatePlatform](../models/syncacquireresponsestackstateplatform.md)                   | :heavy_check_mark:                                                                                                   | Represents the target cloud platform.                                                                                |
| `resourcePrefix`                                                                                                     | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | A prefix used for resource naming to ensure uniqueness across deployments.                                           |
| `resources`                                                                                                          | Record<string, [models.SyncAcquireResponseStackStateResources](../models/syncacquireresponsestackstateresources.md)> | :heavy_check_mark:                                                                                                   | The state of individual resources, keyed by resource ID.                                                             |