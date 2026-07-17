# SyncAcquireResponseDeploymentCurrentRelease

Release metadata

Identifies a specific release version and includes the stack definition.
The deployment engine uses this to track which release is currently deployed
and which is the target.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCurrentRelease } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCurrentRelease = {
  stack: {
    id: "<id>",
    resources: {},
  },
};
```

## Fields

| Field                                                                                                                                               | Type                                                                                                                                                | Required                                                                                                                                            | Description                                                                                                                                         |
| --------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                                                       | *string*                                                                                                                                            | :heavy_minus_sign:                                                                                                                                  | Short description of the release                                                                                                                    |
| `releaseId`                                                                                                                                         | *string*                                                                                                                                            | :heavy_minus_sign:                                                                                                                                  | Release ID (e.g., rel_xyz). `None` for an observe deployment, which has no<br/>Alien-assigned release — the platform resolves a release from `version`. |
| `stack`                                                                                                                                             | [models.SyncAcquireResponseDeploymentCurrentReleaseStack](../models/syncacquireresponsedeploymentcurrentreleasestack.md)                            | :heavy_check_mark:                                                                                                                                  | A bag of resources, unaware of any cloud.                                                                                                           |
| `version`                                                                                                                                           | *string*                                                                                                                                            | :heavy_minus_sign:                                                                                                                                  | Version string (e.g., 2.1.0)                                                                                                                        |