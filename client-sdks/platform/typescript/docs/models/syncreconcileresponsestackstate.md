# SyncReconcileResponseStackState

Represents the collective state of all resources in a stack, including platform and pending actions.

## Example Usage

```typescript
import { SyncReconcileResponseStackState } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseStackState = {
  platform: "test",
  resourcePrefix: "<value>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      status: "delete-failed",
      type: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `platform`                                                                                                               | [models.SyncReconcileResponseStackStatePlatform](../models/syncreconcileresponsestackstateplatform.md)                   | :heavy_check_mark:                                                                                                       | Represents the target cloud platform.                                                                                    |
| `resourcePrefix`                                                                                                         | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | A prefix used for resource naming to ensure uniqueness across deployments.                                               |
| `resources`                                                                                                              | Record<string, [models.SyncReconcileResponseStackStateResources](../models/syncreconcileresponsestackstateresources.md)> | :heavy_check_mark:                                                                                                       | The state of individual resources, keyed by resource ID.                                                                 |