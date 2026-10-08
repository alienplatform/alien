# OperationsConfig

Operations an Operator installed without a release declares in its environment. Setting values are never sent.

## Example Usage

```typescript
import { OperationsConfig } from "@alienplatform/platform-api/models";

let value: OperationsConfig = {};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `custom`                                                                                       | [models.SyncReconcileRequestCustom](../models/syncreconcilerequestcustom.md)[]                 | :heavy_minus_sign:                                                                             | Published custom plugins, pinned to exact versions.                                            |
| `plugins`                                                                                      | Record<string, [models.SyncReconcileRequestPlugins](../models/syncreconcilerequestplugins.md)> | :heavy_minus_sign:                                                                             | Built-in plugins, by plugin name.                                                              |