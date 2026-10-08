# PersistImportedDeploymentRequestPreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackOperations } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackOperations = {};
```

## Fields

| Field                                                                                                                                            | Type                                                                                                                                             | Required                                                                                                                                         | Description                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `custom`                                                                                                                                         | [models.PersistImportedDeploymentRequestPreparedStackCustom](../models/persistimporteddeploymentrequestpreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                                                               | Published custom plugins, pinned to exact versions.                                                                                              |
| `plugins`                                                                                                                                        | Record<string, [models.PersistImportedDeploymentRequestPreparedStackPlugins](../models/persistimporteddeploymentrequestpreparedstackplugins.md)> | :heavy_minus_sign:                                                                                                                               | Built-in plugins, by plugin name.                                                                                                                |