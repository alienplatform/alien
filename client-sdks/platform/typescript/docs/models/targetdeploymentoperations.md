# TargetDeploymentOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { TargetDeploymentOperations } from "@alienplatform/platform-api/models";

let value: TargetDeploymentOperations = {};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `custom`                                                                               | [models.TargetDeploymentCustom](../models/targetdeploymentcustom.md)[]                 | :heavy_minus_sign:                                                                     | Published custom plugins, pinned to exact versions.                                    |
| `plugins`                                                                              | Record<string, [models.TargetDeploymentPlugins](../models/targetdeploymentplugins.md)> | :heavy_minus_sign:                                                                     | Built-in plugins, by plugin name.                                                      |