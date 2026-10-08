# DeploymentPreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { DeploymentPreparedStackOperations } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackOperations = {};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `custom`                                                                                             | [models.DeploymentPreparedStackCustom](../models/deploymentpreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                   | Published custom plugins, pinned to exact versions.                                                  |
| `plugins`                                                                                            | Record<string, [models.DeploymentPreparedStackPlugins](../models/deploymentpreparedstackplugins.md)> | :heavy_minus_sign:                                                                                   | Built-in plugins, by plugin name.                                                                    |