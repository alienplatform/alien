# CurrentReleaseStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { CurrentReleaseStack } from "@alienplatform/platform-api/models";

let value: CurrentReleaseStack = {
  id: "<id>",
  resources: {},
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `id`                                                                                     | *string*                                                                                 | :heavy_check_mark:                                                                       | Unique identifier for the stack                                                          |
| `inputs`                                                                                 | [models.CurrentReleaseInput](../models/currentreleaseinput.md)[]                         | :heavy_minus_sign:                                                                       | Input definitions required before setup or deployment can proceed.                       |
| `permissions`                                                                            | [models.CurrentReleasePermissions](../models/currentreleasepermissions.md)               | :heavy_minus_sign:                                                                       | Combined permissions configuration that contains both profiles and management            |
| `resources`                                                                              | Record<string, [models.CurrentReleaseResources](../models/currentreleaseresources.md)>   | :heavy_check_mark:                                                                       | Map of resource IDs to their configurations and lifecycle settings                       |
| `supportedPlatforms`                                                                     | [models.CurrentReleaseSupportedPlatform](../models/currentreleasesupportedplatform.md)[] | :heavy_minus_sign:                                                                       | Which platforms this stack supports. When None, all platforms are supported.             |