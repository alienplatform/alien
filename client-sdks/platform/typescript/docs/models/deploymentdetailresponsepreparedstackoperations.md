# DeploymentDetailResponsePreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackOperations } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackOperations = {};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `custom`                                                                                                                         | [models.DeploymentDetailResponsePreparedStackCustom](../models/deploymentdetailresponsepreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                                               | Published custom plugins, pinned to exact versions.                                                                              |
| `plugins`                                                                                                                        | Record<string, [models.DeploymentDetailResponsePreparedStackPlugins](../models/deploymentdetailresponsepreparedstackplugins.md)> | :heavy_minus_sign:                                                                                                               | Built-in plugins, by plugin name.                                                                                                |