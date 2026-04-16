# SyncAcquireResponseCurrentReleaseResources

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseResources } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseResources = {
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
  lifecycle: "live-on-setup",
};
```

## Fields

| Field                                                                                                                                                                                             | Type                                                                                                                                                                                              | Required                                                                                                                                                                                          | Description                                                                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `config`                                                                                                                                                                                          | [models.SyncAcquireResponseCurrentReleaseConfig](../models/syncacquireresponsecurrentreleaseconfig.md)                                                                                            | :heavy_check_mark:                                                                                                                                                                                | Resource that can hold any resource type in the Alien system. All resources share common 'type' and 'id' fields with additional type-specific properties.                                         |
| `dependencies`                                                                                                                                                                                    | [models.SyncAcquireResponseCurrentReleaseDependency](../models/syncacquireresponsecurrentreleasedependency.md)[]                                                                                  | :heavy_check_mark:                                                                                                                                                                                | Additional dependencies for this resource beyond those defined in the resource itself.<br/>The total dependencies are: resource.get_dependencies() + this list                                    |
| `lifecycle`                                                                                                                                                                                       | [models.SyncAcquireResponseCurrentReleaseLifecycle](../models/syncacquireresponsecurrentreleaselifecycle.md)                                                                                      | :heavy_check_mark:                                                                                                                                                                                | Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.                                                                                                  |
| `remoteAccess`                                                                                                                                                                                    | *boolean*                                                                                                                                                                                         | :heavy_minus_sign:                                                                                                                                                                                | Enable remote bindings for this resource (BYOB use case).<br/>When true, binding params are synced to StackState's `remote_binding_params`.<br/>Default: false (prevents sensitive data in synced state). |