# SyncAcquireResponsePreparedStackResources

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackResources } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackResources = {
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
  lifecycle: "frozen",
};
```

## Fields

| Field                                                                                                                                                                                             | Type                                                                                                                                                                                              | Required                                                                                                                                                                                          | Description                                                                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `config`                                                                                                                                                                                          | [models.SyncAcquireResponsePreparedStackConfig](../models/syncacquireresponsepreparedstackconfig.md)                                                                                              | :heavy_check_mark:                                                                                                                                                                                | Resource that can hold any resource type in the Alien system. All resources share common 'type' and 'id' fields with additional type-specific properties.                                         |
| `dependencies`                                                                                                                                                                                    | [models.SyncAcquireResponsePreparedStackDependency](../models/syncacquireresponsepreparedstackdependency.md)[]                                                                                    | :heavy_check_mark:                                                                                                                                                                                | Additional dependencies for this resource beyond those defined in the resource itself.<br/>The total dependencies are: resource.get_dependencies() + this list                                    |
| `lifecycle`                                                                                                                                                                                       | [models.SyncAcquireResponsePreparedStackLifecycle](../models/syncacquireresponsepreparedstacklifecycle.md)                                                                                        | :heavy_check_mark:                                                                                                                                                                                | Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.                                                                                                  |
| `remoteAccess`                                                                                                                                                                                    | *boolean*                                                                                                                                                                                         | :heavy_minus_sign:                                                                                                                                                                                | Enable remote bindings for this resource (BYOB use case).<br/>When true, binding params are synced to StackState's `remote_binding_params`.<br/>Default: false (prevents sensitive data in synced state). |