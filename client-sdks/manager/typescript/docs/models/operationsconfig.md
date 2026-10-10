# OperationsConfig

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { OperationsConfig } from "@alienplatform/manager-api/models";

let value: OperationsConfig = {};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `custom`                                                                             | [models.CustomPluginOperationsConfig](../models/custompluginoperationsconfig.md)[]   | :heavy_minus_sign:                                                                   | Published custom plugins, pinned to exact versions.                                  |
| `plugins`                                                                            | Record<string, [models.PluginOperationsConfig](../models/pluginoperationsconfig.md)> | :heavy_minus_sign:                                                                   | Built-in plugins, by plugin name.                                                    |
