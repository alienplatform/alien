# TargetReleaseOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { TargetReleaseOperations } from "@alienplatform/platform-api/models";

let value: TargetReleaseOperations = {};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `custom`                                                                         | [models.TargetReleaseCustom](../models/targetreleasecustom.md)[]                 | :heavy_minus_sign:                                                               | Published custom plugins, pinned to exact versions.                              |
| `plugins`                                                                        | Record<string, [models.TargetReleasePlugins](../models/targetreleaseplugins.md)> | :heavy_minus_sign:                                                               | Built-in plugins, by plugin name.                                                |