# DeploymentStatePreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { DeploymentStatePreparedStackOperations } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackOperations = {};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `custom`                                                                                                       | [models.DeploymentStatePreparedStackCustom](../models/deploymentstatepreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                             | Published custom plugins, pinned to exact versions.                                                            |
| `plugins`                                                                                                      | Record<string, [models.DeploymentStatePreparedStackPlugins](../models/deploymentstatepreparedstackplugins.md)> | :heavy_minus_sign:                                                                                             | Built-in plugins, by plugin name.                                                                              |