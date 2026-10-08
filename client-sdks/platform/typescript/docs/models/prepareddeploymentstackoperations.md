# PreparedDeploymentStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { PreparedDeploymentStackOperations } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackOperations = {};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `custom`                                                                                             | [models.PreparedDeploymentStackCustom](../models/prepareddeploymentstackcustom.md)[]                 | :heavy_minus_sign:                                                                                   | Published custom plugins, pinned to exact versions.                                                  |
| `plugins`                                                                                            | Record<string, [models.PreparedDeploymentStackPlugins](../models/prepareddeploymentstackplugins.md)> | :heavy_minus_sign:                                                                                   | Built-in plugins, by plugin name.                                                                    |