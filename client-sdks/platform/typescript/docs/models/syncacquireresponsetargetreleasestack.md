# SyncAcquireResponseTargetReleaseStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseStack = {
  id: "<id>",
  resources: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                         | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Unique identifier for the stack                                                                                              |
| `inputs`                                                                                                                     | [models.SyncAcquireResponseTargetReleaseInput](../models/syncacquireresponsetargetreleaseinput.md)[]                         | :heavy_minus_sign:                                                                                                           | Input definitions required before setup or deployment can proceed.                                                           |
| `permissions`                                                                                                                | [models.SyncAcquireResponseTargetReleasePermissions](../models/syncacquireresponsetargetreleasepermissions.md)               | :heavy_minus_sign:                                                                                                           | Combined permissions configuration that contains both profiles and management                                                |
| `resources`                                                                                                                  | Record<string, [models.SyncAcquireResponseTargetReleaseResources](../models/syncacquireresponsetargetreleaseresources.md)>   | :heavy_check_mark:                                                                                                           | Map of resource IDs to their configurations and lifecycle settings                                                           |
| `supportedPlatforms`                                                                                                         | [models.SyncAcquireResponseTargetReleaseSupportedPlatform](../models/syncacquireresponsetargetreleasesupportedplatform.md)[] | :heavy_minus_sign:                                                                                                           | Which platforms this stack supports. When None, all platforms are supported.                                                 |