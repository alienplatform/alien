# SyncReconcileRequestTargetRelease

Release metadata

Identifies a specific release version and includes the stack definition.
The deployment engine uses this to track which release is currently deployed
and which is the target.

## Example Usage

```typescript
import { SyncReconcileRequestTargetRelease } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetRelease = {
  releaseId: "<id>",
  stack: {
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
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `description`                                                                                        | *string*                                                                                             | :heavy_minus_sign:                                                                                   | Short description of the release                                                                     |
| `releaseId`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | Release ID (e.g., rel_xyz)                                                                           |
| `stack`                                                                                              | [models.SyncReconcileRequestTargetReleaseStack](../models/syncreconcilerequesttargetreleasestack.md) | :heavy_check_mark:                                                                                   | A bag of resources, unaware of any cloud.                                                            |
| `version`                                                                                            | *string*                                                                                             | :heavy_minus_sign:                                                                                   | Version string (e.g., 2.1.0)                                                                         |