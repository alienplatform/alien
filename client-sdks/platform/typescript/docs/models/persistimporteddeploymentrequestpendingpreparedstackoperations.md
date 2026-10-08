# PersistImportedDeploymentRequestPendingPreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackOperations } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPendingPreparedStackOperations = {};
```

## Fields

| Field                                                                                                                                                          | Type                                                                                                                                                           | Required                                                                                                                                                       | Description                                                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `custom`                                                                                                                                                       | [models.PersistImportedDeploymentRequestPendingPreparedStackCustom](../models/persistimporteddeploymentrequestpendingpreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                                                                             | Published custom plugins, pinned to exact versions.                                                                                                            |
| `plugins`                                                                                                                                                      | Record<string, [models.PersistImportedDeploymentRequestPendingPreparedStackPlugins](../models/persistimporteddeploymentrequestpendingpreparedstackplugins.md)> | :heavy_minus_sign:                                                                                                                                             | Built-in plugins, by plugin name.                                                                                                                              |