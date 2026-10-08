# DeploymentDetailResponsePendingPreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackOperations } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackOperations = {};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `custom`                                                                                                                                       | [models.DeploymentDetailResponsePendingPreparedStackCustom](../models/deploymentdetailresponsependingpreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                                                             | Published custom plugins, pinned to exact versions.                                                                                            |
| `plugins`                                                                                                                                      | Record<string, [models.DeploymentDetailResponsePendingPreparedStackPlugins](../models/deploymentdetailresponsependingpreparedstackplugins.md)> | :heavy_minus_sign:                                                                                                                             | Built-in plugins, by plugin name.                                                                                                              |