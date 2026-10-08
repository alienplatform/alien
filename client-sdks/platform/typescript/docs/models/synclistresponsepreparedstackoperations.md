# SyncListResponsePreparedStackOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { SyncListResponsePreparedStackOperations } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackOperations = {};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `custom`                                                                                                         | [models.SyncListResponsePreparedStackCustom](../models/synclistresponsepreparedstackcustom.md)[]                 | :heavy_minus_sign:                                                                                               | Published custom plugins, pinned to exact versions.                                                              |
| `plugins`                                                                                                        | Record<string, [models.SyncListResponsePreparedStackPlugins](../models/synclistresponsepreparedstackplugins.md)> | :heavy_minus_sign:                                                                                               | Built-in plugins, by plugin name.                                                                                |