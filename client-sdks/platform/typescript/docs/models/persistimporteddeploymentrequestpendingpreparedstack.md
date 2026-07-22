# PersistImportedDeploymentRequestPendingPreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStack } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPendingPreparedStack = {
  id: "<id>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      dependencies: [],
      lifecycle: "live",
    },
  },
};
```

## Fields

| Field                                                                                                                                                                | Type                                                                                                                                                                 | Required                                                                                                                                                             | Description                                                                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                                 | *string*                                                                                                                                                             | :heavy_check_mark:                                                                                                                                                   | Unique identifier for the stack                                                                                                                                      |
| `inputs`                                                                                                                                                             | [models.PersistImportedDeploymentRequestPendingPreparedStackInput](../models/persistimporteddeploymentrequestpendingpreparedstackinput.md)[]                         | :heavy_minus_sign:                                                                                                                                                   | Input definitions required before setup or deployment can proceed.                                                                                                   |
| `permissions`                                                                                                                                                        | [models.PersistImportedDeploymentRequestPendingPreparedStackPermissions](../models/persistimporteddeploymentrequestpendingpreparedstackpermissions.md)               | :heavy_minus_sign:                                                                                                                                                   | Combined permissions configuration that contains both profiles and management                                                                                        |
| `resources`                                                                                                                                                          | Record<string, [models.PersistImportedDeploymentRequestPendingPreparedStackResources](../models/persistimporteddeploymentrequestpendingpreparedstackresources.md)>   | :heavy_check_mark:                                                                                                                                                   | Map of resource IDs to their configurations and lifecycle settings                                                                                                   |
| `supportedPlatforms`                                                                                                                                                 | [models.PersistImportedDeploymentRequestPendingPreparedStackSupportedPlatform](../models/persistimporteddeploymentrequestpendingpreparedstacksupportedplatform.md)[] | :heavy_minus_sign:                                                                                                                                                   | Which platforms this stack supports. When None, all platforms are supported.                                                                                         |
