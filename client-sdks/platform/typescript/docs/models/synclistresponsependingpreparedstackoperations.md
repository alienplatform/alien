# SyncListResponsePendingPreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackOperations } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackOperations = {};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `custom`                                                                                                                       | [models.SyncListResponsePendingPreparedStackCustom](../models/synclistresponsependingpreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                                             | Published custom plugins, pinned to exact versions.                                                                            |
| `plugins`                                                                                                                      | Record<string, [models.SyncListResponsePendingPreparedStackPlugins](../models/synclistresponsependingpreparedstackplugins.md)> | :heavy_minus_sign:                                                                                                             | Built-in plugins, by plugin name.                                                                                              |