# PersistImportedDeploymentRequestPreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStack } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStack = {
  id: "<id>",
  resources: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                         | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Unique identifier for the stack                                                                                              |
| `permissions`                                                                                                                | [models.PersistImportedDeploymentRequestPermissions](../models/persistimporteddeploymentrequestpermissions.md)               | :heavy_minus_sign:                                                                                                           | Combined permissions configuration that contains both profiles and management                                                |
| `resources`                                                                                                                  | Record<string, [models.PersistImportedDeploymentRequestResources](../models/persistimporteddeploymentrequestresources.md)>   | :heavy_check_mark:                                                                                                           | Map of resource IDs to their configurations and lifecycle settings                                                           |
| `supportedPlatforms`                                                                                                         | [models.PersistImportedDeploymentRequestSupportedPlatform](../models/persistimporteddeploymentrequestsupportedplatform.md)[] | :heavy_minus_sign:                                                                                                           | Which platforms this stack supports. When None, all platforms are supported.                                                 |