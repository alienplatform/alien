# CreateChildDeploymentRequestStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { CreateChildDeploymentRequestStack } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestStack = {
  id: "<id>",
  resources: {},
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                 | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Unique identifier for the stack                                                                                      |
| `inputs`                                                                                                             | [models.CreateChildDeploymentRequestInput](../models/createchilddeploymentrequestinput.md)[]                         | :heavy_minus_sign:                                                                                                   | Input definitions required before setup or deployment can proceed.                                                   |
| `permissions`                                                                                                        | [models.CreateChildDeploymentRequestPermissions](../models/createchilddeploymentrequestpermissions.md)               | :heavy_minus_sign:                                                                                                   | Combined permissions configuration that contains both profiles and management                                        |
| `resources`                                                                                                          | Record<string, [models.CreateChildDeploymentRequestResources](../models/createchilddeploymentrequestresources.md)>   | :heavy_check_mark:                                                                                                   | Map of resource IDs to their configurations and lifecycle settings                                                   |
| `supportedPlatforms`                                                                                                 | [models.CreateChildDeploymentRequestSupportedPlatform](../models/createchilddeploymentrequestsupportedplatform.md)[] | :heavy_minus_sign:                                                                                                   | Which platforms this stack supports. When None, all platforms are supported.                                         |