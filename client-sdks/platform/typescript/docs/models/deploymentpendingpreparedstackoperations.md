# DeploymentPendingPreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { DeploymentPendingPreparedStackOperations } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackOperations = {};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `custom`                                                                                                           | [models.DeploymentPendingPreparedStackCustom](../models/deploymentpendingpreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                                 | Published custom plugins, pinned to exact versions.                                                                |
| `plugins`                                                                                                          | Record<string, [models.DeploymentPendingPreparedStackPlugins](../models/deploymentpendingpreparedstackplugins.md)> | :heavy_minus_sign:                                                                                                 | Built-in plugins, by plugin name.                                                                                  |