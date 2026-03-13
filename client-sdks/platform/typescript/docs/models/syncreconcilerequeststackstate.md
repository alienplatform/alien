# SyncReconcileRequestStackState

Represents the collective state of all resources in a stack, including platform and pending actions.

## Example Usage

```typescript
import { SyncReconcileRequestStackState } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStackState = {
  platform: "local",
  resourcePrefix: "<value>",
  resources: {},
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                             | [models.SyncReconcileRequestStackStatePlatform](../models/syncreconcilerequeststackstateplatform.md)                   | :heavy_check_mark:                                                                                                     | Represents the target cloud platform.                                                                                  |
| `resourcePrefix`                                                                                                       | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | A prefix used for resource naming to ensure uniqueness across deployments.                                             |
| `resources`                                                                                                            | Record<string, [models.SyncReconcileRequestStackStateResources](../models/syncreconcilerequeststackstateresources.md)> | :heavy_check_mark:                                                                                                     | The state of individual resources, keyed by resource ID.                                                               |