# DeploymentStatePendingPreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackOperations } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackOperations = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `custom`                                                                                                                     | [models.DeploymentStatePendingPreparedStackCustom](../models/deploymentstatependingpreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                                           | Published custom plugins, pinned to exact versions.                                                                          |
| `plugins`                                                                                                                    | Record<string, [models.DeploymentStatePendingPreparedStackPlugins](../models/deploymentstatependingpreparedstackplugins.md)> | :heavy_minus_sign:                                                                                                           | Built-in plugins, by plugin name.                                                                                            |